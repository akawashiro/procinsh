use crate::http_server::process::maps::MemoryMap;
use object::{Object, ObjectSection, ObjectSegment, ObjectSymbol};
use std::{
    collections::HashMap,
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
/// Parsed ELF data. The DWARF loader has its own synchronization for lazy data.
pub(in crate::http_server::process::snapshot) struct ElfSymbols {
    pub(in crate::http_server::process::snapshot) unwind:
        framehop::ExplicitModuleSectionInfo<Arc<[u8]>>,
    pub(super) segments: Vec<(u64, u64, u64)>,
    pub(super) symbols: Vec<(u64, u64, String)>,
    pub(super) dwarf: Option<Mutex<addr2line::Loader>>,
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

        let range = |name| {
            file.section_by_name(name)
                .and_then(|s| Some(s.address()..s.address().checked_add(s.size())?))
        };
        let data = |name| {
            file.section_by_name(name)
                .and_then(|s| s.uncompressed_data().ok())
                .map(|bytes| Arc::<[u8]>::from(bytes.as_ref()))
        };
        // framehop selects one format per module and prefers .eh_frame. When
        // -fno-unwind-tables produces .debug_frame, CRT-only .eh_frame would
        // shadow the application's CFI. Prefer its complete debug frame data.
        let debug_frame = data(".debug_frame");
        let prefer_debug = debug_frame.is_some();
        let unwind = framehop::ExplicitModuleSectionInfo {
            text_svma: range(".text"),
            got_svma: range(".got"),
            eh_frame_svma: range(".eh_frame"),
            eh_frame: (!prefer_debug).then(|| data(".eh_frame")).flatten(),
            eh_frame_hdr_svma: range(".eh_frame_hdr"),
            eh_frame_hdr: (!prefer_debug).then(|| data(".eh_frame_hdr")).flatten(),
            debug_frame,
            ..Default::default()
        };
        Some(Self {
            unwind,
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
pub(in crate::http_server::process::snapshot) struct ElfCache {
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
    paths.into_iter().find_map(|path| {
        let meta = fs::metadata(&path).ok()?;
        (meta.ino() == map.inode
            && libc::major(meta.dev()) == map.device.major
            && libc::minor(meta.dev()) == map.device.minor)
            .then_some((path, meta))
    })
}

impl ElfCache {
    /// Locate the mapped file by identity and reuse or load its ELF/DWARF data.
    /// Performs I/O; call only after the target has resumed. Returned data may
    /// outlive eviction and can be resolved after releasing the cache lock.
    pub(in crate::http_server::process::snapshot) fn get(
        &mut self,
        pid: i32,
        map: &MemoryMap,
    ) -> Option<Arc<ElfSymbols>> {
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

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
