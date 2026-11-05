//! Native kernel-event values and CPU key decoding.
//!
//! # Interface
//! - [`RawActivity`] (`pub`): `struct RawActivity`.
//! - [`CpuCounters`] (`pub`): `struct CpuCounters`.
//! - [`ProcessKey`] (`pub`): `struct ProcessKey`.
//! - [`SignalSample`] (`pub`): `struct SignalSample`.
//! - [`IpcEvent`] (`pub`): `struct IpcEvent`.
//! - [`FileActivity`] (`pub`): `struct FileActivity`.
//! - [`CpuActivity`] (`pub`): `struct CpuActivity`.
//! - [`IpcActivity`] (`pub`): `struct IpcActivity`.
//! - [`SignalEvent`] (`pub`): `struct SignalEvent`.
//! - [`process_key`] (`pub`): `fn process_key(bytes: &[u8]) -> Option<ProcessKey>`.
//! - [`u64_at`] (`pub(crate)`): `fn u64_at(bytes: &[u8], offset: usize) -> Option<u64>`.
use crate::ipc::{
    model::resource::{FileIdentity, IpcIdentity},
    schema::ProcessId,
};

#[derive(Clone, Default)]
pub struct RawActivity {
    pub files: Vec<FileActivity>,
    pub ipc: Vec<IpcEvent>,
    pub signals: Vec<SignalSample>,
    pub cpu: CpuCounters,
}

#[derive(Clone, Default)]
pub struct CpuCounters {
    pub now: u64,
    pub current: Vec<Vec<u8>>,
    /// Nonzero per-CPU cumulative runtime/switch pairs; CPU indices are in `current`.
    pub totals: Vec<(Vec<u8>, Vec<[u64; 2]>)>,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ProcessKey {
    pub start: u64,
    pub pid: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct SignalSample {
    pub timestamp_ns: u64,
    pub src_start: u64,
    pub dst_start: u64,
    pub src_pid: u32,
    pub dst_pid: u32,
    pub signal: i32,
}

#[derive(Clone, Copy)]
pub struct IpcEvent {
    pub start: u64,
    pub inode: u64,
    pub device: u64,
    pub bytes: u64,
    pub pid: u32,
    pub kind: u32,
    pub write: bool,
    pub worker: bool,
}

#[derive(Clone, Debug)]
pub struct FileActivity {
    pub process_id: ProcessId,
    pub file: FileIdentity,
    pub path: Option<String>,
    pub write: bool,
    pub bytes: u64,
    pub count: u64,
}

#[derive(Clone)]
pub struct CpuActivity {
    pub process_id: ProcessId,
    pub runtime_ns: u64,
    pub switches: u64,
    pub running_threads: usize,
    pub cpus: Vec<usize>,
}

#[derive(Clone)]
pub struct IpcActivity {
    pub process_id: ProcessId,
    pub resource: IpcIdentity,
    pub write: bool,
    pub bytes: u64,
    pub count: u64,
}

#[derive(Clone, Debug)]
pub struct SignalEvent {
    pub timestamp_ns: u64,
    pub src_pid: u32,
    pub dst_pid: u32,
    pub signal: i32,
    pub source_id: ProcessId,
    pub destination_id: ProcessId,
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

pub fn process_key(bytes: &[u8]) -> Option<ProcessKey> {
    Some(ProcessKey {
        start: u64_at(bytes, 0)?,
        pid: u32_at(bytes, 8)?,
    })
}
