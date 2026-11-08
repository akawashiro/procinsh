//! Parse bounded ELF/DWARF bytes and retain symbol and unwind data in main.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`ElfSymbols`] | `pub` | `struct ElfSymbols` |
//! | [`ElfCache`] | `pub` | `struct ElfCache` |
//! | [`parse_reader_elf_symbols`] | `pub(super)` | `fn parse_reader_elf_symbols<'a, R: `[`object::read::ReadRef`]`<'a>>( bytes: R, ) -> `[`Option`]`<`[`ElfSymbols`]`>` |
//! | [`cache_elf_symbols_from_image`] | `pub` | `fn cache_elf_symbols_from_image( state: &mut `[`ElfCache`]`, map: &`[`MemoryMap`]`, file: &`[`super::image::ElfImage`]`, ) -> `[`Option`]`<`[`Arc`]`<`[`ElfSymbols`]`>>` |
//! | [`lookup_cached_elf_file_identity`] | `pub` | `fn lookup_cached_elf_file_identity(state: &`[`ElfCache`]`, map: &`[`MemoryMap`]`) -> `[`Option`]`<`[`ElfFileId`]`>` |
//! | [`lookup_elf_symbols`] | `pub` | `fn lookup_elf_symbols(state: &`[`ElfCache`]`, map: &`[`MemoryMap`]`) -> `[`Option`]`<`[`Arc`]`<`[`ElfSymbols`]`>>` |

use crate::ipc::model::memory::MemoryMap;
use object::{Object, ObjectSection, ObjectSegment, ObjectSymbol};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
type DwarfReader = gimli::EndianArcSlice<gimli::RunTimeEndian>;

/// Parsed ELF data from verified bytes acquired by the helper. The DWARF context uses owned,
/// bounded section bytes and never opens supplementary or split debug files.
pub struct ElfSymbols {
    /// Shared ELF section bytes used by the unwinder.
    pub unwind: framehop::ExplicitModuleSectionInfo<Arc<[u8]>>,
    pub segments: Vec<(u64, u64, u64)>,
    pub symbols: Vec<(u64, u64, String)>,
    pub dwarf: Option<Mutex<addr2line::Context<DwarfReader>>>,
}

#[cfg(test)]
pub fn parse_elf_symbols(bytes: &[u8]) -> Option<ElfSymbols> {
    parse_reader_elf_symbols(bytes)
}

pub(super) fn parse_reader_elf_symbols<'a, R: object::read::ReadRef<'a>>(
    bytes: R,
) -> Option<ElfSymbols> {
    if bytes.len().ok()? > MAX_FILE_SIZE {
        return None;
    }
    let file = object::File::parse(bytes).ok()?;
    if file.format() != object::BinaryFormat::Elf {
        return None;
    }
    // Validate decompression sizes before invoking any decompressor. The
    // aggregate also bounds retained DWARF/unwind section storage.
    let mut expanded = 0_u64;
    for section in file.sections() {
        if !super::image::needed_section(section.name().ok()?) {
            continue;
        }
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
    Some(ElfSymbols {
        unwind,
        segments,
        symbols,
        dwarf,
    })
}

const MAX_FILES: usize = 64;
const MAX_FILE_SIZE: u64 = 128 * 1024 * 1024;
const MAX_SECTION_SIZE: u64 = 32 * 1024 * 1024;
const MAX_EXPANDED_SIZE: u64 = 64 * 1024 * 1024;
const MAX_SYMBOLS: usize = 262_144;
const MAX_UNITS: usize = 16_384;
const MAX_CACHE_BYTES: u64 = 128 * 1024 * 1024;

use crate::ipc::elf::ElfFileId;
#[cfg(test)]
pub struct ElfFile {
    pub identity: ElfFileId,
    pub bytes: Vec<u8>,
}
type MappedFileId = (u32, u32, u64);

/// Main-owned parsed modules keyed by verified file versions and mapping identities.
/// The cache bounds source-file bytes to 128 MiB and parsed entries to 64;
/// it also retains at most 64 rejected versions without their file bytes.
/// Unwinder-held `Arc`s may outlive cache eviction.
#[derive(Default)]
pub struct ElfCache {
    entries: HashMap<ElfFileId, Arc<ElfSymbols>>,
    mappings: HashMap<MappedFileId, ElfFileId>,
    failures: HashMap<MappedFileId, ElfFileId>,
}
#[cfg(test)]
pub fn cache_elf_symbols_from_bytes(
    state: &mut ElfCache,
    map: &MemoryMap,
    file: ElfFile,
) -> Option<Arc<ElfSymbols>> {
    if file.identity.2 != file.bytes.len() as u64 {
        return None;
    }
    insert_reader_elf_cache(state, map, file.identity, file.bytes.as_slice())
}

pub fn cache_elf_symbols_from_image(
    state: &mut ElfCache,
    map: &MemoryMap,
    file: &super::image::ElfImage,
) -> Option<Arc<ElfSymbols>> {
    insert_reader_elf_cache(state, map, file.identity, file)
}

fn insert_reader_elf_cache<'a, R: object::read::ReadRef<'a>>(
    state: &mut ElfCache,
    map: &MemoryMap,
    key: ElfFileId,
    bytes: R,
) -> Option<Arc<ElfSymbols>> {
    if key.1 != map.inode
        || libc::major(key.0) != map.device.major
        || libc::minor(key.0) != map.device.minor
    {
        return None;
    }
    let elf = if let Some(elf) = state.entries.get(&key) {
        Arc::clone(elf)
    } else {
        let Some(parsed) = parse_reader_elf_symbols(bytes) else {
            state
                .mappings
                .remove(&(map.device.major, map.device.minor, map.inode));
            if state.failures.len() >= MAX_FILES {
                state.failures.clear();
            }
            state
                .failures
                .insert((map.device.major, map.device.minor, map.inode), key);
            return None;
        };
        state
            .failures
            .remove(&(map.device.major, map.device.minor, map.inode));
        let elf = Arc::new(parsed);
        if state.entries.len() >= MAX_FILES
            || state
                .entries
                .keys()
                .map(|key| key.2)
                .sum::<u64>()
                .saturating_add(key.2)
                > MAX_CACHE_BYTES
        {
            state.entries.clear();
            state.mappings.clear();
        }
        state.entries.insert(key, Arc::clone(&elf));
        elf
    };
    state
        .mappings
        .insert((map.device.major, map.device.minor, map.inode), key);
    Some(elf)
}

/// Version known to main, including unsupported files cached as failures.
pub fn lookup_cached_elf_file_identity(state: &ElfCache, map: &MemoryMap) -> Option<ElfFileId> {
    let key = (map.device.major, map.device.minor, map.inode);
    state
        .mappings
        .get(&key)
        .filter(|id| state.entries.contains_key(id))
        .or_else(|| state.failures.get(&key))
        .copied()
}

pub fn lookup_elf_symbols(state: &ElfCache, map: &MemoryMap) -> Option<Arc<ElfSymbols>> {
    state
        .entries
        .get(
            state
                .mappings
                .get(&(map.device.major, map.device.minor, map.inode))?,
        )
        .cloned()
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
