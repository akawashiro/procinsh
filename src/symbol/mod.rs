use crate::{
    process::maps::MemoryMap,
    snapshot::unwind_fp::{SourceFrame, StackFrame},
};
use object::{Object, ObjectSegment, ObjectSymbol};
use std::{collections::HashMap, fs, os::unix::fs::MetadataExt, path::PathBuf};

struct Elf {
    segments: Vec<(u64, u64, u64)>, // file offset, file size, virtual address
    symbols: Vec<(u64, u64, String)>,
    dwarf: Option<addr2line::Loader>,
}
#[derive(Default)]
pub struct Symbolizer {
    cache: HashMap<(u64, u64, u64, i64, i64), Elf>,
}

fn matching_file(pid: i32, map: &MemoryMap) -> Option<(PathBuf, fs::Metadata)> {
    let mut paths = vec![
        PathBuf::from(format!(
            "/proc/{pid}/map_files/{:x}-{:x}",
            map.start, map.end
        )),
        PathBuf::from(format!("/proc/{pid}/exe")),
    ];
    if let Some(path) = map
        .pathname
        .as_deref()
        .filter(|p| p.starts_with('/') && !p.ends_with(" (deleted)"))
    {
        paths.push(PathBuf::from(format!("/proc/{pid}/root{path}")));
    }
    let (major, minor) = map.device.split_once(':')?;
    let major = u64::from_str_radix(major, 16).ok()?;
    let minor = u64::from_str_radix(minor, 16).ok()?;
    paths.into_iter().find_map(|path| {
        let meta = fs::metadata(&path).ok()?;
        (meta.ino() == map.inode
            && libc::major(meta.dev()) as u64 == major
            && libc::minor(meta.dev()) as u64 == minor)
            .then_some((path, meta))
    })
}
impl Symbolizer {
    pub fn resolve(
        &mut self,
        pid: i32,
        maps: &[MemoryMap],
        frame: &mut StackFrame,
        return_address: bool,
    ) {
        let address = frame.address.saturating_sub(u64::from(return_address));
        let Some(map) = maps.iter().find(|m| m.contains(address) && m.inode != 0) else {
            return;
        };
        let Some((path, meta)) = matching_file(pid, map) else {
            return;
        };
        let key = (
            meta.dev(),
            meta.ino(),
            meta.len(),
            meta.mtime(),
            meta.mtime_nsec(),
        );
        if !self.cache.contains_key(&key) {
            if self.cache.len() >= 64 {
                self.cache.clear();
            }
            // Bound per-file allocation. Missing/oversized debug data leaves raw addresses usable.
            if meta.len() > 512 * 1024 * 1024 {
                return;
            }
            let Ok(bytes) = fs::read(&path) else { return };
            let Ok(file) = object::File::parse(bytes.as_slice()) else {
                return;
            };
            let segments = file
                .segments()
                .map(|s| {
                    let (offset, size) = s.file_range();
                    (offset, size, s.address())
                })
                .collect();
            let mut symbols: Vec<_> = file
                .symbols()
                .chain(file.dynamic_symbols())
                .filter(|s| s.is_definition() && s.kind() == object::SymbolKind::Text)
                .filter_map(|s| Some((s.address(), s.size(), s.name().ok()?.to_owned())))
                .collect();
            symbols.sort_by_key(|s| s.0);
            let dwarf = addr2line::Loader::new(&path).ok();
            self.cache.insert(
                key,
                Elf {
                    segments,
                    symbols,
                    dwarf,
                },
            );
        }
        let elf = &self.cache[&key];
        let Some(offset) = address
            .checked_sub(map.start)
            .and_then(|v| v.checked_add(map.file_offset))
        else {
            return;
        };
        let page = crate::process::procfs::page_size();
        let Some(&(file_offset, _, virtual_address)) =
            elf.segments.iter().find(|&&(off, size, _)| {
                offset >= off / page * page
                    && offset < off.saturating_add(size).div_ceil(page) * page
            })
        else {
            return;
        };
        let relative = (offset as i128 + virtual_address as i128 - file_offset as i128).try_into();
        let Ok(relative): Result<u64, _> = relative else {
            return;
        };
        if let Some((start, _, name)) = elf.symbols.iter().rev().find(|(start, size, _)| {
            *start <= relative && (*size == 0 || relative < start.saturating_add(*size))
        }) {
            frame.symbol = Some(name.clone());
            frame.symbol_offset = Some(format!("0x{:x}", relative - start));
        }
        if let Some(loader) = &elf.dwarf {
            if let Ok(mut iter) = loader.find_frames(relative) {
                while let Ok(Some(f)) = iter.next() {
                    let function = f
                        .function
                        .and_then(|f| f.demangle().ok().map(|v| v.into_owned()));
                    let file = f.location.as_ref().and_then(|l| l.file.map(str::to_owned));
                    let line = f.location.and_then(|l| l.line);
                    frame.inline_frames.push(SourceFrame {
                        function,
                        file,
                        line,
                    });
                }
            }
            if let Some(source) = frame.inline_frames.first() {
                if source.function.is_some() {
                    frame.symbol = source.function.clone();
                }
                frame.source_file = source.file.clone();
                frame.line = source.line;
            } else if let Ok(Some(location)) = loader.find_location(relative) {
                frame.source_file = location.file.map(str::to_owned);
                frame.line = location.line;
            }
        }
    }
}
