//! Read bounded process mapping observations.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`read_maps`] | `pub(in crate::privileged_helper)` | `fn read_maps(pid: i32) -> `[`Result`]`<`[`Vec`]`<`[`MemoryMap`]`>>` |
//! | [`read_smaps`] | `pub(super)` | `fn read_smaps(pid: i32) -> `[`Result`]`<`[`Vec`]`<`[`SmapsEntry`]`>>` |
//! | [`read_memory_rollup`] | `pub(super)` | `fn read_memory_rollup(pid: i32) -> `[`Option`]`<`[`MemoryRollup`]`>` |
//! | [`collect_memory_maps`] | `pub(in crate::privileged_helper)` | `fn collect_memory_maps(id: `[`ProcessId`]`) -> `[`Result`]`<`[`MemoryMaps`]`>` |

use super::{permission_help, procfs::read_procfs_text, validate_process_identity};
use crate::ipc::{
    model::memory::{MemoryMap, SmapsEntry, parse_map, parse_smaps},
    schema::ProcessId,
};
use crate::{ipc::acquisition::MemoryMaps, privileged_helper::timestamp_ms};
use anyhow::Result;

pub(in crate::privileged_helper) fn read_maps(pid: i32) -> Result<Vec<MemoryMap>> {
    read_procfs_text(&format!("/proc/{pid}/maps"))?
        .lines()
        .map(parse_map)
        .collect()
}

/// Reads smaps, falling back to maps without usage measurements if smaps cannot be read.
pub(super) fn read_smaps(pid: i32) -> Result<Vec<SmapsEntry>> {
    if let Ok(text) = read_procfs_text(&format!("/proc/{pid}/smaps")) {
        return parse_smaps(&text);
    }
    Ok(read_maps(pid)?.into_iter().map(SmapsEntry::from).collect())
}

use crate::ipc::acquisition::MemoryRollup;

pub(super) fn read_memory_rollup(pid: i32) -> Option<MemoryRollup> {
    let f = super::procfs::read_procfs_fields(&format!("/proc/{pid}/smaps_rollup")).ok()?;
    let bytes = |k| super::procfs::parse_procfs_u64_field(&f, k).and_then(|v| v.checked_mul(1024));
    Some(MemoryRollup {
        rss_bytes: bytes("Rss"),
        pss_bytes: bytes("Pss"),
        private_bytes: bytes("Private_Clean")
            .zip(bytes("Private_Dirty"))
            .map(|(a, b)| a + b),
    })
}

pub(in crate::privileged_helper) fn collect_memory_maps(id: ProcessId) -> Result<MemoryMaps> {
    validate_process_identity(id)?;
    let result = read_smaps(id.pid);
    let rollup = read_memory_rollup(id.pid);
    validate_process_identity(id)?;
    let (maps, error, captured_at) = match result {
        Ok(maps) => (maps, None, Some(timestamp_ms())),
        Err(e) => (Vec::new(), Some(permission_help("memory maps", e)), None),
    };
    Ok(MemoryMaps {
        process_id: id,
        maps,
        error,
        captured_at,
        rollup,
    })
}
