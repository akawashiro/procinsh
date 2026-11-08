//! Mapping values, classification and parsing of captured maps and smaps.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`MemoryKind`] | `pub(crate)` | `enum MemoryKind` |
//! | [`MemoryMap`] | `pub(crate)` | `struct MemoryMap` |
//! | [`SmapsEntry`] | `pub(crate)` | `struct SmapsEntry` |
//! | [`MappingSnapshot`] | `pub(crate)` | `struct MappingSnapshot` |
//! | [`memory_map_contains`] | `pub(crate)` | `fn memory_map_contains(state: &`[`MemoryMap`]`, address: u64) -> bool` |
//! | [`kind_memory_map`] | `pub(crate)` | `fn kind_memory_map(state: &`[`MemoryMap`]`) -> `[`MemoryKind`] |
//! | [`parse_map`] | `pub(crate)` | `fn parse_map(line: &str) -> `[`Result`]`<`[`MemoryMap`]`>` |
//! | [`parse_smaps`] | `pub(crate)` | `fn parse_smaps(text: &str) -> `[`Result`]`<`[`Vec`]`<`[`SmapsEntry`]`>>` |

use crate::ipc::model::resource::DeviceId;
use anyhow::{Context, Result, ensure};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MemoryKind {
    Integer,
    Stack,
    Heap,
    SharedLibrary,
    Executable,
    File,
    Anonymous,
}
/// Structural mapping without RSS/PSS measurements.
/// Addresses, offsets and inodes retain their integer representation in IPC.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MemoryMap {
    pub(crate) start: u64,
    pub(crate) end: u64,
    pub(crate) readable: bool,
    pub(crate) writable: bool,
    pub(crate) executable: bool,
    pub(crate) private: bool,
    pub(crate) file_offset: u64,
    pub(crate) device: DeviceId,
    pub(crate) inode: u64,
    pub(crate) pathname: Option<String>,
}

/// A mapping with optional resident/proportional usage measured from smaps.
/// The structural mapping and optional usage measurements are distinct fields.
#[derive(Clone, Debug)]
pub(crate) struct SmapsEntry {
    pub(crate) mapping: MemoryMap,
    pub(crate) rss_bytes: Option<u64>,
    pub(crate) pss_bytes: Option<u64>,
}
impl From<MemoryMap> for SmapsEntry {
    fn from(mapping: MemoryMap) -> Self {
        Self {
            mapping,
            rss_bytes: None,
            pss_bytes: None,
        }
    }
}

pub(crate) fn memory_map_contains(state: &MemoryMap, address: u64) -> bool {
    state.start <= address && address < state.end
}

pub(crate) fn kind_memory_map(state: &MemoryMap) -> MemoryKind {
    match state.pathname.as_deref() {
        Some(p) if p.starts_with("[stack") => MemoryKind::Stack,
        Some("[heap]") => MemoryKind::Heap,
        Some(p) if p.contains(".so") => MemoryKind::SharedLibrary,
        _ if state.executable => MemoryKind::Executable,
        Some(p) if p.starts_with('/') => MemoryKind::File,
        _ => MemoryKind::Anonymous,
    }
}

pub(crate) fn parse_map(line: &str) -> Result<MemoryMap> {
    // Split only the first five columns: file names may contain spaces.
    let mut rest = line.trim();
    let mut columns = Vec::new();
    for _ in 0..5 {
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        ensure!(end > 0, "maps: missing field");
        columns.push(&rest[..end]);
        rest = rest[end..].trim_start();
    }
    let (start, end) = columns[0].split_once('-').context("maps: invalid range")?;
    let permissions = columns[1];
    ensure!(permissions.len() == 4, "maps: invalid permissions");
    let start = u64::from_str_radix(start, 16)?;
    let end = u64::from_str_radix(end, 16)?;
    ensure!(start < end, "maps: empty range");
    Ok(MemoryMap {
        start,
        end,
        readable: permissions.as_bytes()[0] == b'r',
        writable: permissions.as_bytes()[1] == b'w',
        executable: permissions.as_bytes()[2] == b'x',
        private: permissions.as_bytes()[3] == b'p',
        file_offset: u64::from_str_radix(columns[2], 16)?,
        device: {
            let (major, minor) = columns[3].split_once(':').context("maps: invalid device")?;
            DeviceId {
                major: u32::from_str_radix(major, 16)?,
                minor: u32::from_str_radix(minor, 16)?,
            }
        },
        inode: columns[4].parse()?,
        pathname: (!rest.is_empty()).then(|| rest.to_owned()),
    })
}

pub(crate) fn parse_smaps(text: &str) -> Result<Vec<SmapsEntry>> {
    let mut maps: Vec<SmapsEntry> = Vec::new();
    for line in text.lines() {
        if line
            .split_whitespace()
            .next()
            .is_some_and(|word| word.contains('-'))
        {
            maps.push(parse_map(line)?.into());
        } else if let Some(map) = maps.last_mut()
            && let Some((key, value)) = line.split_once(':')
        {
            let bytes = value
                .split_whitespace()
                .next()
                .and_then(|v| v.parse::<u64>().ok())
                .and_then(|v| v.checked_mul(1024));
            match key {
                "Rss" => map.rss_bytes = bytes,
                "Pss" => map.pss_bytes = bytes,
                _ => {}
            }
        }
    }
    Ok(maps)
}

/// One captured mapping context shared by all thread samples from that epoch.
#[derive(Clone, Debug)]
pub(crate) struct MappingSnapshot {
    pub(crate) epoch: u64,
    pub(crate) maps: Vec<MemoryMap>,
}

crate::ipc::codec::wire_enum!(MemoryKind{0=>Integer,1=>Stack,2=>Heap,3=>SharedLibrary,4=>Executable,5=>File,6=>Anonymous});

crate::ipc::codec::wire_struct!(MemoryMap {
    start,
    end,
    readable,
    writable,
    executable,
    private,
    file_offset,
    device,
    inode,
    pathname
});

crate::ipc::codec::wire_struct!(SmapsEntry {
    mapping,
    rss_bytes,
    pss_bytes
});

crate::ipc::codec::wire_struct!(MappingSnapshot { epoch, maps });
