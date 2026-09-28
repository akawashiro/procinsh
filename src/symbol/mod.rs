//! ELF acquisition and caching are separate from address conversion and resolution.
//!
//! Call [`ElfCache::get`] after the target resumes, release the cache lock, then
//! use [`elf_address`] and [`resolve_frame`]. Resolution never mutates its input
//! frame or the cache. addr2line may lazily load DWARF internally; that work is
//! serialized per ELF, not under the cache lock.

use crate::{process::maps::MemoryMap, stack::SourceFrame};
use object::{Object, ObjectSegment, ObjectSymbol};
use std::{
    collections::HashMap,
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

/// Symbol and source information for one ELF address.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SymbolInfo {
    pub name: Option<String>,
    pub offset: Option<u64>,
    pub file: Option<String>,
    pub line: Option<u32>,
    pub inline_frames: Vec<SourceFrame>,
}

/// Parsed ELF data. The DWARF loader has its own synchronization for lazy data.
pub struct ElfSymbols {
    segments: Vec<(u64, u64, u64)>,
    symbols: Vec<(u64, u64, String)>,
    dwarf: Option<Mutex<addr2line::Loader>>,
}

impl ElfSymbols {
    fn load(path: &Path) -> Option<Self> {
        let bytes = fs::read(path).ok()?;
        let file = object::File::parse(bytes.as_slice()).ok()?;
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
        let dwarf = addr2line::Loader::new(path).ok().map(Mutex::new);

        Some(Self {
            segments,
            symbols,
            dwarf,
        })
    }
}

type FileIdentity = (u64, u64, u64, i64, i64);
const MAX_FILES: usize = 64;
const MAX_FILE_SIZE: u64 = 512 * 1024 * 1024;

/// Bounded cache keyed by device, inode, size, and nanosecond modification time.
#[derive(Default)]
pub struct ElfCache {
    entries: HashMap<FileIdentity, Arc<ElfSymbols>>,
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

impl ElfCache {
    /// Locate the mapped file by identity and reuse or load its ELF/DWARF data.
    /// Performs I/O; call only after the target has resumed. Returned data may
    /// outlive eviction and can be resolved after releasing the cache lock.
    pub fn get(&mut self, pid: i32, map: &MemoryMap) -> Option<Arc<ElfSymbols>> {
        let (path, meta) = matching_file(pid, map)?;
        let key = (
            meta.dev(),
            meta.ino(),
            meta.len(),
            meta.mtime(),
            meta.mtime_nsec(),
        );
        if let Some(elf) = self.entries.get(&key) {
            return Some(Arc::clone(elf));
        }
        if meta.len() > MAX_FILE_SIZE {
            return None;
        }
        let elf = Arc::new(ElfSymbols::load(&path)?);
        if self.entries.len() >= MAX_FILES {
            self.entries.clear();
        }
        self.entries.insert(key, Arc::clone(&elf));
        Some(elf)
    }
}

/// Correct a saved return address before selecting its mapping.
/// The currently executing instruction must be passed with `return_address = false`.
pub fn instruction_address(address: u64, return_address: bool) -> u64 {
    address.saturating_sub(u64::from(return_address))
}

/// Convert a corrected runtime address into an ELF address, accounting for the
/// mapping offset, PIE/ASLR load bias, and page-aligned segment mappings.
pub fn elf_address(address: u64, map: &MemoryMap, elf: &ElfSymbols, page: u64) -> Option<u64> {
    if page == 0 || !map.contains(address) {
        return None;
    }
    let offset = address
        .checked_sub(map.start)?
        .checked_add(map.file_offset)?;
    let &(file_offset, _, virtual_address) = elf.segments.iter().find(|&&(off, size, _)| {
        offset >= off / page * page
            && (offset as u128) < (off as u128 + size as u128).div_ceil(page as u128) * page as u128
    })?;
    (offset as i128 + virtual_address as i128 - file_offset as i128)
        .try_into()
        .ok()
}

/// Resolve an ELF address without changing a frame or the ELF cache.
/// Returns `None` only when no symbol, source location, or inline frame is found.
/// addr2line may perform lazy debug-file I/O under the ELF's private lock.
pub fn resolve_frame(address: u64, elf: &ElfSymbols) -> Option<SymbolInfo> {
    let mut info = SymbolInfo::default();
    if let Some((start, _, name)) = elf.symbols.iter().rev().find(|(start, size, _)| {
        *start <= address && (*size == 0 || address < start.saturating_add(*size))
    }) {
        info.name = Some(name.clone());
        info.offset = Some(address - start);
    }
    if let Some(loader) = &elf.dwarf {
        let loader = loader.lock().unwrap_or_else(|e| e.into_inner());
        if let Ok(mut iter) = loader.find_frames(address) {
            while let Ok(Some(f)) = iter.next() {
                let function = f
                    .function
                    .and_then(|f| f.demangle().ok().map(|v| v.into_owned()));
                let file = f.location.as_ref().and_then(|l| l.file.map(str::to_owned));
                let line = f.location.and_then(|l| l.line);
                info.inline_frames.push(SourceFrame {
                    function,
                    file,
                    line,
                });
            }
        }
        if let Some(source) = info.inline_frames.first() {
            if source.function.is_some() {
                info.name = source.function.clone();
            }
            info.file = source.file.clone();
            info.line = source.line;
        } else if let Ok(Some(location)) = loader.find_location(address) {
            info.file = location.file.map(str::to_owned);
            info.line = location.line;
        }
    }
    (info.name.is_some()
        || info.file.is_some()
        || info.line.is_some()
        || !info.inline_frames.is_empty())
    .then_some(info)
}

#[cfg(test)]
mod tests;
