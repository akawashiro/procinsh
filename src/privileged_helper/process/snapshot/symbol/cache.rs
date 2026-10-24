use crate::privileged_helper::process::maps::MemoryMap;
use object::{Object, ObjectSection, ObjectSegment, ObjectSymbol};
#[cfg(test)]
use std::path::Path;
use std::{
    collections::HashMap,
    fs,
    io::Read,
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::PathBuf,
    sync::{Arc, Mutex},
};

type DwarfReader = gimli::EndianArcSlice<gimli::RunTimeEndian>;

/// Parsed ELF data from one verified descriptor. The DWARF context uses owned,
/// bounded section bytes and never opens supplementary or split debug files.
pub(in crate::privileged_helper::process::snapshot) struct ElfSymbols {
    /// Shared ELF section bytes used by the unwinder.
    pub(in crate::privileged_helper::process::snapshot) unwind:
        framehop::ExplicitModuleSectionInfo<Arc<[u8]>>,
    pub(super) segments: Vec<(u64, u64, u64)>,
    pub(super) symbols: Vec<(u64, u64, String)>,
    pub(super) dwarf: Option<Mutex<addr2line::Context<DwarfReader>>>,
}

impl ElfSymbols {
    #[cfg(test)]
    fn load(path: &Path) -> Option<Self> {
        Self::load_file(&fs::File::open(path).ok()?)
    }

    fn load_file(descriptor: &fs::File) -> Option<Self> {
        let before = descriptor.metadata().ok()?;
        if !before.is_file() || before.len() > MAX_FILE_SIZE {
            return None;
        }
        let mut bytes = Vec::new();
        descriptor
            .take(MAX_FILE_SIZE + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        let after = descriptor.metadata().ok()?;
        if bytes.len() as u64 > MAX_FILE_SIZE || file_identity(&before) != file_identity(&after) {
            return None;
        }
        let file = object::File::parse(bytes.as_slice()).ok()?;
        if file.format() != object::BinaryFormat::Elf {
            return None;
        }
        // Validate decompression sizes before invoking any decompressor. The
        // aggregate also bounds retained DWARF/unwind section storage.
        let mut expanded = 0_u64;
        for section in file.sections() {
            let compression = section.compressed_file_range().ok()?;
            // object's zstd path grows its output with read_to_end and accepts
            // an untrusted decoder window. Only its fixed-output zlib path is
            // suitable here; unsupported compression falls back to raw frames.
            if !matches!(
                compression.format,
                object::CompressionFormat::None | object::CompressionFormat::Zlib
            ) {
                return None;
            }
            let size = compression.uncompressed_size;
            if size > MAX_SECTION_SIZE {
                return None;
            }
            expanded = expanded.checked_add(size)?;
            if expanded > MAX_EXPANDED_SIZE {
                return None;
            }
        }
        if file
            .symbols()
            .count()
            .checked_add(file.dynamic_symbols().count())?
            > MAX_SYMBOLS
        {
            return None;
        }
        let segments = file
            .segments()
            .map(|s| {
                let (offset, size) = s.file_range();
                (offset, size, s.address())
            })
            .collect();
        let mut symbols = Vec::new();
        let mut name_bytes = 0_usize;
        for symbol in file
            .symbols()
            .chain(file.dynamic_symbols())
            .filter(|s| s.is_definition() && s.kind() == object::SymbolKind::Text)
        {
            let Some(name) = symbol.name().ok() else {
                continue;
            };
            if name.len() > 4096 {
                continue;
            }
            name_bytes = name_bytes.checked_add(name.len())?;
            if name_bytes > 8 * 1024 * 1024 {
                return None;
            }
            symbols.push((symbol.address(), symbol.size(), name.to_owned()));
        }
        symbols.sort_by_key(|s| s.0);
        let endian = if file.is_little_endian() {
            gimli::RunTimeEndian::Little
        } else {
            gimli::RunTimeEndian::Big
        };
        let dwarf = gimli::Dwarf::load(|id| {
            let bytes = file
                .section_by_name(id.name())
                .map(|s| s.uncompressed_data())
                .transpose()
                .map_err(|_| gimli::Error::Io)?
                .unwrap_or_default();
            Ok::<_, gimli::Error>(DwarfReader::new(Arc::from(bytes.as_ref()), endian))
        })
        .ok()
        .and_then(|dwarf| {
            let mut units = dwarf.units();
            let mut count = 0;
            while units.next().ok()?.is_some() {
                count += 1;
                if count > MAX_UNITS {
                    return None;
                }
            }
            addr2line::Context::from_dwarf(dwarf).ok().map(Mutex::new)
        });

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
const MAX_FILE_SIZE: u64 = 128 * 1024 * 1024;
const MAX_SECTION_SIZE: u64 = 32 * 1024 * 1024;
const MAX_EXPANDED_SIZE: u64 = 64 * 1024 * 1024;
const MAX_SYMBOLS: usize = 262_144;
const MAX_UNITS: usize = 16_384;
const MAX_CACHE_BYTES: u64 = 128 * 1024 * 1024;

fn file_identity(meta: &fs::Metadata) -> FileIdentity {
    (
        meta.dev(),
        meta.ino(),
        meta.len(),
        meta.mtime(),
        meta.mtime_nsec(),
    )
}

/// Bounded cache keyed by device, inode, size, and nanosecond modification time.
#[derive(Default)]
pub(in crate::privileged_helper::process::snapshot) struct ElfCache {
    entries: HashMap<FileIdentity, Arc<ElfSymbols>>,
}

fn matching_file(pid: i32, map: &MemoryMap) -> Option<(fs::File, fs::Metadata)> {
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
        // O_PATH pins the selected inode without opening devices or blocking on
        // FIFOs. Only verified regular files may then be opened for reading.
        let pinned = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_PATH | libc::O_CLOEXEC)
            .open(&path)
            .ok()?;
        let meta = pinned.metadata().ok()?;
        if !meta.is_file()
            || meta.ino() != map.inode
            || libc::major(meta.dev()) != map.device.major
            || libc::minor(meta.dev()) != map.device.minor
        {
            return None;
        }
        let descriptor = fs::File::open(format!("/proc/self/fd/{}", pinned.as_raw_fd())).ok()?;
        (file_identity(&descriptor.metadata().ok()?) == file_identity(&meta))
            .then_some((descriptor, meta))
    })
}

impl ElfCache {
    /// Locate the mapped file by identity and reuse or load its ELF/DWARF data.
    /// Performs I/O; call only after the target has resumed. Returned data may
    /// outlive cache eviction.
    pub(in crate::privileged_helper::process::snapshot) fn get(
        &mut self,
        pid: i32,
        map: &MemoryMap,
    ) -> Option<Arc<ElfSymbols>> {
        let (descriptor, meta) = matching_file(pid, map)?;
        let key = file_identity(&meta);
        if let Some(elf) = self.entries.get(&key) {
            return Some(Arc::clone(elf));
        }
        if meta.len() > MAX_FILE_SIZE {
            return None;
        }
        let elf = Arc::new(ElfSymbols::load_file(&descriptor)?);
        if self.entries.len() >= MAX_FILES
            || self
                .entries
                .keys()
                .map(|key| key.2)
                .sum::<u64>()
                .saturating_add(meta.len())
                > MAX_CACHE_BYTES
        {
            self.entries.clear();
        }
        self.entries.insert(key, Arc::clone(&elf));
        Some(elf)
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
