//! Read bounded process mapping observations.

//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`read_maps`] | `pub(super)` | `fn read_maps(pid: i32) -> Result<Vec<MemoryMap>>` |
//! | [`read_smaps`] | `pub(super)` | `fn read_smaps(pid: i32) -> Result<Vec<SmapsEntry>>` |
//! | [`MemoryRollup`] | `pub(super)` | `struct MemoryRollup` |
//! | [`rollup`] | `pub(super)` | `fn rollup(pid: i32) -> Option<MemoryRollup>` |
//! | [`MemoryKind`] | `pub(in crate::privileged_helper)` | `enum MemoryKind` |
//! | [`MappingSnapshot`] | `pub(in crate::privileged_helper)` | `struct MappingSnapshot` |
//! | [`MemoryMap`] | `pub(in crate::privileged_helper)` | `struct MemoryMap` |
//! | [`SmapsEntry`] | `pub(in crate::privileged_helper)` | `struct SmapsEntry` |
//! | [`parse_map`] | `pub(in crate::privileged_helper)` | `fn parse_map(line: &str) -> Result<MemoryMap>` |
//! | [`parse_smaps`] | `pub(in crate::privileged_helper)` | `fn parse_smaps(text: &str) -> Result<Vec<SmapsEntry>>` |

use super::procfs::read_text;
pub(in crate::privileged_helper) use crate::ipc::model::memory::*;
use anyhow::Result;
pub(super) fn read_maps(pid: i32) -> Result<Vec<MemoryMap>> {
    read_text(&format!("/proc/{pid}/maps"))?
        .lines()
        .map(parse_map)
        .collect()
}

/// Reads smaps, falling back to maps without usage measurements if smaps cannot be read.
pub(super) fn read_smaps(pid: i32) -> Result<Vec<SmapsEntry>> {
    if let Ok(text) = read_text(&format!("/proc/{pid}/smaps")) {
        return parse_smaps(&text);
    }
    Ok(read_maps(pid)?.into_iter().map(SmapsEntry::from).collect())
}

pub use crate::ipc::acquisition::MemoryRollup;

pub(super) fn rollup(pid: i32) -> Option<MemoryRollup> {
    let f = super::procfs::fields(&format!("/proc/{pid}/smaps_rollup")).ok()?;
    let bytes = |k| super::procfs::field_u64(&f, k).and_then(|v| v.checked_mul(1024));
    Some(MemoryRollup {
        rss_bytes: bytes("Rss"),
        pss_bytes: bytes("Pss"),
        private_bytes: bytes("Private_Clean")
            .zip(bytes("Private_Dirty"))
            .map(|(a, b)| a + b),
    })
}
