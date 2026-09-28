use super::*;
use crate::http_server::process::maps::parse_map;
use std::process::Command;

fn empty_elf() -> ElfSymbols {
    ElfSymbols {
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
fn absent_and_symbol_only_results_are_repeatable() {
    let mut elf = empty_elf();
    assert_eq!(resolve_frame(0x100, &elf), None);
    elf.symbols.push((0x100, 0x20, "function".into()));
    let expected = SymbolInfo {
        name: Some("function".into()),
        offset: Some(3),
        ..Default::default()
    };
    assert_eq!(resolve_frame(0x103, &elf), Some(expected.clone()));
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
        let dir =
            std::env::temp_dir().join(format!("procinsh-symbol-tests-{}", std::process::id()));
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
    let pid = std::process::id() as i32;
    let mut cache = ElfCache::default();
    let miss = cache.get(pid, &map).unwrap();
    let hit = cache.get(pid, &map).unwrap();
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
    assert_eq!(Some(info.clone()), resolve_frame(address, &miss));

    // No ELF symbols: DWARF alone must still produce a result.
    let mut dwarf_only = ElfSymbols::load(&path).unwrap();
    dwarf_only.symbols.clear();
    let dwarf_info = resolve_frame(address, &dwarf_only).unwrap();
    assert_eq!(dwarf_info.inline_frames, info.inline_frames);
    assert_eq!(dwarf_info.offset, None);

    // Line tables without function DIEs or an ELF symbol table.
    let lines_path = fixture.compile("-gline-tables-only", "lines");
    let mut lines = ElfSymbols::load(&lines_path).unwrap();
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
    let reloaded = cache.get(pid, &map).unwrap();
    assert_eq!(cache.entries.len(), 1);
    assert!(!Arc::ptr_eq(&miss, &reloaded));
    assert_eq!(Some(info), resolve_frame(address, &reloaded));
    assert!(resolve_frame(address, &miss).is_some());

    let mut wrong_identity = map.clone();
    wrong_identity.inode += 1;
    assert!(cache.get(pid, &wrong_identity).is_none());
    let large_path = fixture.0.join("oversized");
    fs::File::create(&large_path)
        .unwrap()
        .set_len(MAX_FILE_SIZE + 1)
        .unwrap();
    assert!(cache.get(pid, &file_map(&large_path)).is_none());
    assert_eq!(cache.entries.len(), 1);
}
