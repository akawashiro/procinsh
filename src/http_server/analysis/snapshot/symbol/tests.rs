use super::super::{SymbolInfo, elf_address, instruction_address, resolve_frame};
use super::*;
use crate::http_server::model::memory::parse_map;
use std::process::Command;
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::Arc,
};

fn empty_elf() -> ElfSymbols {
    ElfSymbols {
        unwind: Default::default(),
        segments: vec![(0x1100, 0x800, 0x2100)],
        symbols: vec![],
        dwarf: None,
    }
}

#[test]
fn addresses_account_for_load_bias_offsets_pages_and_returns() {
    let elf = empty_elf();
    for base in [0x400000, 0x7f1234000000] {
        let map = parse_map(&format!(
            "{base:x}-{:x} r-xp 1000 00:00 1 /fixture",
            base + 0x1000
        ))
        .unwrap();
        assert_eq!(elf_address(base + 0x123, &map, &elf, 4096), Some(0x2123));
        // A return address at the end belongs to the preceding mapping.
        assert_eq!(
            elf_address(instruction_address(map.end, true), &map, &elf, 4096),
            Some(0x2fff)
        );
        assert_eq!(
            elf_address(instruction_address(map.end, false), &map, &elf, 4096),
            None
        );
        assert_eq!(elf_address(base - 1, &map, &elf, 4096), None);
        assert_eq!(elf_address(base, &map, &elf, 0), None);
    }
    assert_eq!(instruction_address(0, true), 0);
    let mut map = parse_map("1000-2000 r-xp 3000 00:00 1 /fixture").unwrap();
    assert_eq!(elf_address(0x1000, &map, &elf, 4096), None);
    map.file_offset = u64::MAX;
    assert_eq!(elf_address(0x1001, &map, &elf, 4096), None);
}

#[test]
fn absent_and_symbol_only_frames_respect_symbol_bounds() {
    let mut elf = empty_elf();
    assert_eq!(resolve_frame(0x100, &elf), None);
    elf.symbols.push((0x100, 0x20, "function".into()));
    let expected = SymbolInfo {
        name: Some("function".into()),
        offset: Some(3),
        ..Default::default()
    };
    assert_eq!(resolve_frame(0x103, &elf), Some(expected));
    assert_eq!(resolve_frame(0x120, &elf), None);
}

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let serial = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "procinsh-symbol-tests-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&dir).unwrap();
        Self(dir)
    }

    fn compile(&self, debug: &str, name: &str) -> PathBuf {
        let source = self.0.join("fixture.c");
        fs::write(
            &source,
            r#"
static inline __attribute__((always_inline)) int inner(int x) {
    volatile int y = x + 7;
    return y * 3;
}
__attribute__((noinline)) int outer(int x) { return inner(x) + 1; }
int main(int argc, char **argv) { return outer(argc); }
"#,
        )
        .unwrap();
        let path = self.0.join(name);
        let output = Command::new("clang")
            .args([debug, "-O2", "-fPIE", "-pie"])
            .arg(&source)
            .arg("-o")
            .arg(&path)
            .output()
            .expect("clang is required for DWARF fixtures");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        path
    }
}

fn file_map(path: &Path) -> MemoryMap {
    let meta = fs::metadata(path).unwrap();
    parse_map(&format!(
        "1000-2000 r-xp 0 {:x}:{:x} {} {}",
        libc::major(meta.dev()),
        libc::minor(meta.dev()),
        meta.ino(),
        path.display()
    ))
    .unwrap()
}

#[test]
fn dwarf_inline_source_only_and_cache_lifecycle() {
    let fixture = Fixture::new();
    let path = fixture.compile("-g", "full");
    let map = file_map(&path);
    let mut cache = ElfCache::default();
    let miss = load_cached_elf_symbols(&mut cache, &map).unwrap();
    let hit = load_cached_elf_symbols(&mut cache, &map).unwrap();
    assert!(Arc::ptr_eq(&miss, &hit));
    let (start, size, _) = miss.symbols.iter().find(|s| s.2 == "outer").unwrap();
    let address = (*start..start + size)
        .find(|&a| resolve_frame(a, &miss).is_some_and(|i| i.inline_frames.len() >= 2))
        .expect("inlined inner and outer DWARF frames");
    let info = resolve_frame(address, &miss).unwrap();
    assert_eq!(info.inline_frames[0].function.as_deref(), Some("inner"));
    assert!(info.file.as_ref().unwrap().ends_with("fixture.c"));
    assert!(info.line.is_some());
    assert_eq!(Some(info.clone()), resolve_frame(address, &hit));

    // No ELF symbols: DWARF alone must still produce a result.
    let mut dwarf_only = load_elf_symbols(&path).unwrap();
    dwarf_only.symbols.clear();
    let dwarf_info = resolve_frame(address, &dwarf_only).unwrap();
    assert_eq!(dwarf_info.inline_frames, info.inline_frames);
    assert_eq!(dwarf_info.offset, None);

    // Line tables without function DIEs or an ELF symbol table.
    let lines_path = fixture.compile("-gline-tables-only", "lines");
    let mut lines = load_elf_symbols(&lines_path).unwrap();
    let line_address = lines.symbols.iter().find(|s| s.2 == "main").unwrap().0;
    lines.symbols.clear();
    let source = resolve_frame(line_address, &lines).unwrap();
    assert_eq!(source.name, None);
    assert!(source.file.unwrap().ends_with("fixture.c"));
    assert!(source.line.is_some());

    // Eviction does not invalidate data already held by a resolver.
    cache.entries.clear();
    for i in 0..MAX_FILES {
        cache
            .entries
            .insert((0, i as u64, 0, 0, 0), Arc::clone(&miss));
    }
    let reloaded = load_cached_elf_symbols(&mut cache, &map).unwrap();
    assert!(cache.entries.len() <= MAX_FILES);
    assert_eq!(Some(info), resolve_frame(address, &reloaded));
    assert!(resolve_frame(address, &miss).is_some());

    let mut wrong_identity = map.clone();
    wrong_identity.inode += 1;
    assert!(load_cached_elf_symbols(&mut cache, &wrong_identity).is_none());
}

#[test]
fn compressed_debug_sections_reject_decompression_bombs() {
    let fixture = Fixture::new();
    let path = fixture.compile("-g", "compressed");
    let mut bytes = fs::read(&path).unwrap();
    let file = object::File::parse(bytes.as_slice()).unwrap();
    let section = file.section_by_name(".debug_info").unwrap();
    let index = section.index().0;
    let (offset, size) = section.file_range().unwrap();
    assert!(size >= 24);
    let table = u64::from_le_bytes(bytes[40..48].try_into().unwrap()) as usize;
    let entry_size = u16::from_le_bytes(bytes[58..60].try_into().unwrap()) as usize;
    let flags_at = table + index * entry_size + 8;
    let flags = u64::from_le_bytes(bytes[flags_at..flags_at + 8].try_into().unwrap());
    bytes[flags_at..flags_at + 8].copy_from_slice(&(flags | 0x800).to_le_bytes());
    let offset = offset as usize;
    bytes[offset..offset + 4].copy_from_slice(&1_u32.to_le_bytes());
    bytes[offset + 4..offset + 8].fill(0);
    bytes[offset + 8..offset + 16].copy_from_slice(&(MAX_SECTION_SIZE + 1).to_le_bytes());
    bytes[offset + 16..offset + 24].copy_from_slice(&1_u64.to_le_bytes());
    fs::write(&path, bytes).unwrap();
    assert!(load_elf_symbols(&path).is_none());
}

fn load_elf_symbols(path: &Path) -> Option<ElfSymbols> {
    ElfSymbols::parse(&fs::read(path).ok()?)
}

fn load_cached_elf_symbols(cache: &mut ElfCache, map: &MemoryMap) -> Option<Arc<ElfSymbols>> {
    let path = Path::new(map.pathname.as_deref()?);
    let meta = fs::metadata(path).ok()?;
    if meta.len() > MAX_FILE_SIZE {
        return None;
    }
    cache.insert(
        map,
        ElfFile {
            identity: (
                meta.dev(),
                meta.ino(),
                meta.len(),
                meta.mtime(),
                meta.mtime_nsec(),
            ),
            bytes: fs::read(path).ok()?,
        },
    )
}

#[test]
fn rejected_file_versions_do_not_reuse_stale_symbols() {
    let fixture = Fixture::new();
    let path = fixture.compile("-g", "changed");
    let map = file_map(&path);
    let mut cache = ElfCache::default();
    assert!(load_cached_elf_symbols(&mut cache, &map).is_some());
    let old = cache.identity(&map).unwrap();
    fs::write(&path, b"not an ELF").unwrap();
    assert!(load_cached_elf_symbols(&mut cache, &map).is_none());
    assert!(cache.get(&map).is_none());
    let rejected = cache.identity(&map).unwrap();
    assert_ne!(rejected, old);
    assert_eq!(rejected.2, 10);
}

#[test]
fn selected_elf_ranges_preserve_symbols_sources_and_unwind_data() {
    use super::super::image::ElfImage;
    use crate::ipc::elf::{FileChunk, FileRange, MappedFileData};
    let fixture = Fixture::new();
    for flag in ["-g", "-gline-tables-only"] {
        let path = fixture.compile(flag, "sparse");
        let bytes = fs::read(&path).unwrap();
        let identity = (1, 2, bytes.len() as u64, 3, 4);
        let capture = |ranges: Vec<FileRange>| MappedFileData {
            identity,
            chunks: ranges
                .into_iter()
                .map(|range| FileChunk {
                    offset: range.offset,
                    bytes: bytes[range.offset as usize..(range.offset + range.length) as usize]
                        .to_vec(),
                })
                .collect(),
        };
        let mut image = ElfImage::new(capture(vec![FileRange {
            offset: 0,
            length: 64,
        }]))
        .unwrap();
        let mut transferred = 64;
        for stage in 0..3 {
            let ranges = match stage {
                0 => image.tables().unwrap(),
                1 => image.names().unwrap(),
                _ => image.sections().unwrap(),
            };
            transferred += ranges.iter().map(|range| range.length).sum::<u64>();
            image.extend(capture(ranges)).unwrap();
        }
        assert!(
            transferred < bytes.len() as u64,
            "unneeded file bytes should stay in helper"
        );
        let complete = ElfSymbols::parse(&bytes).unwrap();
        let sparse = ElfSymbols::parse_reader(&image).unwrap();
        assert_eq!(complete.symbols, sparse.symbols);
        assert_eq!(complete.segments, sparse.segments);
        assert_eq!(complete.unwind.eh_frame, sparse.unwind.eh_frame);
        assert_eq!(complete.unwind.eh_frame_hdr, sparse.unwind.eh_frame_hdr);
        for (start, size, _) in &complete.symbols {
            for address in *start..start + size {
                assert_eq!(
                    resolve_frame(address, &complete),
                    resolve_frame(address, &sparse)
                );
            }
        }
        let mut changed = capture(vec![]);
        changed.identity.4 += 1;
        assert!(image.extend(changed).is_err());
    }
}
