//! Process operations exposed to HTTP transport and system monitoring.
mod details;
mod discovery;
pub(super) use discovery::{Discovery, ProcessSummary};
use discovery::{summary, users};
mod fds;
mod maps;
mod memory;
mod procfs;
mod signals;
mod sockets;
mod threads;

use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub(super) struct ProcessId {
    pub(super) pid: i32,
    pub(super) start_time_ticks: u64,
}

#[cfg(test)]
pub(super) fn identity(pid: i32) -> Result<ProcessId> {
    ensure!(pid > 0, "PID must be positive");
    let stat = procfs::read_stat(&format!("/proc/{pid}/stat"))?;
    Ok(ProcessId {
        pid,
        start_time_ticks: stat.start_time,
    })
}

pub(super) fn check_identity(id: ProcessId) -> Result<()> {
    ensure!(id.pid > 0, "PID must be positive");
    let stat = match procfs::read_stat(&format!("/proc/{}/stat", id.pid)) {
        Ok(stat) => stat,
        Err(error) => {
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| matches!(e.raw_os_error(), Some(libc::ENOENT) | Some(libc::ESRCH)))
            {
                bail!("Process exited");
            }
            return Err(error);
        }
    };
    ensure!(
        stat.start_time == id.start_time_ticks,
        "Process exited (or PID was reused)"
    );
    ensure!(
        !matches!(stat.state.as_str(), "Z" | "X" | "x"),
        "Process exited"
    );
    Ok(())
}

pub(super) fn timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn permission_help(operation: &str, error: impl std::fmt::Display) -> String {
    format!(
        "{operation}: {error}. If access is denied, check the process owner, kernel.yama.ptrace_scope, and CAP_SYS_PTRACE. sudo is never run automatically."
    )
}

mod monitoring;
mod snapshot;

// These concrete domain types cross the process façade. Their definitions have
// http_server-scoped visibility so they can be re-exported; modules stay private.
pub(super) use maps::MemoryMap;
pub(super) use monitoring::Monitoring;
pub(super) use sockets::SocketInfo;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum SubscribeError {
    Stopped,
    TooManySubscribers,
}

pub(super) fn observation(id: ProcessId) -> Result<impl Serialize> {
    monitoring::observation(id, None)
}
pub(super) fn threads(id: ProcessId) -> Result<impl Serialize> {
    Ok(monitoring::observation(id, None)?.threads)
}
#[derive(Serialize)]
struct MemoryMaps {
    process_id: ProcessId,
    maps: Vec<MemoryMap>,
    error: Option<String>,
    captured_at: Option<u64>,
    rollup: Option<maps::MemoryRollup>,
}
pub(super) fn maps(id: ProcessId) -> Result<impl Serialize> {
    check_identity(id)?;
    let result = maps::read(id.pid, true);
    let rollup = maps::rollup(id.pid);
    check_identity(id)?;
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
pub(super) fn environment(id: ProcessId) -> Result<impl Serialize> {
    details::environment(id)
}
pub(super) fn auxv(id: ProcessId) -> Result<impl Serialize> {
    details::auxv(id)
}
pub(super) fn fds(id: ProcessId) -> Result<impl Serialize> {
    fds::read(id)
}
pub(super) fn signals(id: ProcessId) -> Result<impl Serialize> {
    signals::read(id)
}
pub(super) const MAX_READ: usize = memory::MAX_READ;
pub(super) fn memory(id: ProcessId, address: u64, length: usize) -> Result<impl Serialize> {
    memory::read(id, address, length)
}
#[derive(Default)]
pub(super) struct Snapshotter(snapshot::Snapshotter);
impl Snapshotter {
    pub(super) fn capture(&self, id: ProcessId) -> Result<impl Serialize> {
        self.0.capture(id)
    }
}
// Minimal primitives also used by system-wide monitoring.
pub(super) fn memory_maps(pid: i32) -> Result<Vec<MemoryMap>> {
    maps::read(pid, false)
}
pub(super) fn ticks_per_second() -> f64 {
    procfs::ticks_per_second()
}
pub(super) fn fields(path: &str) -> Result<std::collections::HashMap<String, String>> {
    procfs::fields(path)
}
pub(super) fn socket_text(path: &str) -> Result<String> {
    sockets::read_text(path)
}
pub(super) fn inet_sockets(
    text: &str,
    protocol: &str,
) -> std::collections::HashMap<u64, SocketInfo> {
    sockets::parse_inet(text, protocol)
}
pub(super) fn unix_sockets(text: &str) -> std::collections::HashMap<u64, SocketInfo> {
    sockets::parse_unix(text)
}
pub(super) fn unix_socket_peers(
    deadline: std::time::Instant,
) -> Result<std::collections::HashMap<u64, SocketInfo>> {
    sockets::unix_diag(deadline)
}

#[cfg(test)]
mod test_support;
#[cfg(test)]
pub(super) use test_support::Target as TestTarget;
#[cfg(test)]
mod tests;
