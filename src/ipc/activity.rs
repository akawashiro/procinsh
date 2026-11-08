//! Native kernel-event values and CPU key decoding.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`RawActivity`] | `pub(crate)` | `struct RawActivity` |
//! | [`CpuCounters`] | `pub(crate)` | `struct CpuCounters` |
//! | [`ProcessKey`] | `pub(crate)` | `struct ProcessKey` |
//! | [`SignalSample`] | `pub(crate)` | `struct SignalSample` |
//! | [`IpcEvent`] | `pub(crate)` | `struct IpcEvent` |
//! | [`FileActivity`] | `pub(crate)` | `struct FileActivity` |
//! | [`CpuActivity`] | `pub(crate)` | `struct CpuActivity` |
//! | [`IpcActivity`] | `pub(crate)` | `struct IpcActivity` |
//! | [`SignalEvent`] | `pub(crate)` | `struct SignalEvent` |
//! | [`u64_at`] | `pub(crate)` | `fn u64_at(bytes: &[u8], offset: usize) -> `[`Option`]`<u64>` |
//! | [`process_key`] | `pub(crate)` | `fn process_key(bytes: &[u8]) -> `[`Option`]`<`[`ProcessKey`]`>` |

use crate::ipc::{
    model::resource::{FileIdentity, IpcIdentity},
    schema::ProcessId,
};

#[derive(Clone, Default)]
pub(crate) struct RawActivity {
    pub(crate) files: Vec<FileActivity>,
    pub(crate) ipc: Vec<IpcEvent>,
    pub(crate) signals: Vec<SignalSample>,
    pub(crate) cpu: CpuCounters,
}

#[derive(Clone, Default)]
pub(crate) struct CpuCounters {
    pub(crate) now: u64,
    pub(crate) current: Vec<Vec<u8>>,
    /// Nonzero per-CPU cumulative runtime/switch pairs; CPU indices are in `current`.
    pub(crate) totals: Vec<(Vec<u8>, Vec<[u64; 2]>)>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct ProcessKey {
    pub(crate) start: u64,
    pub(crate) pid: u32,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SignalSample {
    pub(crate) timestamp_ns: u64,
    pub(crate) src_start: u64,
    pub(crate) dst_start: u64,
    pub(crate) src_pid: u32,
    pub(crate) dst_pid: u32,
    pub(crate) signal: i32,
}

#[derive(Clone, Copy)]
pub(crate) struct IpcEvent {
    pub(crate) start: u64,
    pub(crate) inode: u64,
    pub(crate) device: u64,
    pub(crate) bytes: u64,
    pub(crate) pid: u32,
    pub(crate) kind: u32,
    pub(crate) write: bool,
    pub(crate) worker: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct FileActivity {
    pub(crate) process_id: ProcessId,
    pub(crate) file: FileIdentity,
    pub(crate) path: Option<String>,
    pub(crate) write: bool,
    pub(crate) bytes: u64,
    pub(crate) count: u64,
}

#[derive(Clone)]
pub(crate) struct CpuActivity {
    pub(crate) process_id: ProcessId,
    pub(crate) runtime_ns: u64,
    pub(crate) switches: u64,
    pub(crate) running_threads: usize,
    pub(crate) cpus: Vec<usize>,
}

#[derive(Clone)]
pub(crate) struct IpcActivity {
    pub(crate) process_id: ProcessId,
    pub(crate) resource: IpcIdentity,
    pub(crate) write: bool,
    pub(crate) bytes: u64,
    pub(crate) count: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct SignalEvent {
    pub(crate) timestamp_ns: u64,
    pub(crate) src_pid: u32,
    pub(crate) dst_pid: u32,
    pub(crate) signal: i32,
    pub(crate) source_id: ProcessId,
    pub(crate) destination_id: ProcessId,
}

crate::ipc::codec::wire_struct!(RawActivity {
    files,
    ipc,
    signals,
    cpu
});

crate::ipc::codec::wire_struct!(CpuCounters {
    now,
    current,
    totals
});

crate::ipc::codec::wire_struct!(ProcessKey { start, pid });

crate::ipc::codec::wire_struct!(SignalSample {
    timestamp_ns,
    src_start,
    dst_start,
    src_pid,
    dst_pid,
    signal
});

crate::ipc::codec::wire_struct!(IpcEvent {
    start,
    inode,
    device,
    bytes,
    pid,
    kind,
    write,
    worker
});

crate::ipc::codec::wire_struct!(FileActivity {
    process_id,
    file,
    path,
    write,
    bytes,
    count
});

crate::ipc::codec::wire_struct!(CpuActivity {
    process_id,
    runtime_ns,
    switches,
    running_threads,
    cpus
});

crate::ipc::codec::wire_struct!(IpcActivity {
    process_id,
    resource,
    write,
    bytes,
    count
});

crate::ipc::codec::wire_struct!(SignalEvent {
    timestamp_ns,
    src_pid,
    dst_pid,
    signal,
    source_id,
    destination_id
});

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_ne_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

pub(crate) fn u64_at(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_ne_bytes(
        bytes.get(offset..offset + 8)?.try_into().ok()?,
    ))
}

pub(crate) fn process_key(bytes: &[u8]) -> Option<ProcessKey> {
    Some(ProcessKey {
        start: u64_at(bytes, 0)?,
        pid: u32_at(bytes, 8)?,
    })
}
