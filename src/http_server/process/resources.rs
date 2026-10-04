use super::{
    ProcessId, check_identity, details,
    maps::{self, MemoryMap, SmapsEntry},
    monitoring, procfs,
    sockets::{self, SocketInfo},
};
use anyhow::Result;
use serde::Serialize;
use std::time::{SystemTime, UNIX_EPOCH};

pub(in crate::http_server) fn timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub(super) fn permission_help(operation: &str, error: impl std::fmt::Display) -> String {
    format!(
        "{operation}: {error}. If access is denied, check the process owner, kernel.yama.ptrace_scope, and CAP_SYS_PTRACE. sudo is never run automatically."
    )
}

#[derive(Debug, PartialEq, Eq)]
pub(in crate::http_server) enum SubscribeError {
    Stopped,
    TooManySubscribers,
}

pub(in crate::http_server) fn observation(id: ProcessId) -> Result<impl Serialize> {
    Ok(monitoring::initial_observation(
        &monitoring::capture_sample(id)?,
    ))
}

pub(in crate::http_server) fn threads(id: ProcessId) -> Result<impl Serialize> {
    Ok(monitoring::initial_observation(&monitoring::capture_sample(id)?).threads)
}
#[derive(Serialize)]
struct MemoryMaps {
    process_id: ProcessId,
    maps: Vec<SmapsEntry>,
    error: Option<String>,
    captured_at: Option<u64>,
    rollup: Option<maps::MemoryRollup>,
}

pub(in crate::http_server) fn maps(id: ProcessId) -> Result<impl Serialize> {
    check_identity(id)?;
    let result = maps::read_smaps(id.pid);
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

pub(in crate::http_server) fn environment(id: ProcessId) -> Result<impl Serialize> {
    details::environment(id)
}

pub(in crate::http_server) fn auxv(id: ProcessId) -> Result<impl Serialize> {
    details::auxv(id)
}

pub(in crate::http_server) fn fds(id: ProcessId) -> Result<impl Serialize> {
    super::fds::read(id)
}

// Minimal primitives also used by system-wide monitoring.
pub(in crate::http_server) fn memory_maps(pid: i32) -> Result<Vec<MemoryMap>> {
    maps::read_maps(pid)
}

pub(in crate::http_server) fn ticks_per_second() -> f64 {
    procfs::ticks_per_second()
}

pub(in crate::http_server) fn fields(
    path: &str,
) -> Result<std::collections::HashMap<String, String>> {
    procfs::fields(path)
}

pub(in crate::http_server) fn socket_text(path: &str) -> Result<String> {
    sockets::read_text(path)
}

pub(in crate::http_server) fn inet_sockets(
    text: &str,
    protocol: crate::http_server::socket_types::SocketProtocol,
) -> std::collections::HashMap<u64, SocketInfo> {
    sockets::parse_inet(text, protocol)
}

pub(in crate::http_server) fn unix_sockets(
    text: &str,
) -> std::collections::HashMap<u64, SocketInfo> {
    sockets::parse_unix(text)
}

pub(in crate::http_server) fn unix_socket_peers(
    deadline: std::time::Instant,
) -> Result<std::collections::HashMap<u64, SocketInfo>> {
    sockets::unix_diag(deadline)
}
