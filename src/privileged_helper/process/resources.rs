//! Acquire process resources and provide shared procfs access and timestamps.
//!
//! # Interface
//! - [`SubscribeError`] (`pub(in crate::privileged_helper)`): `enum SubscribeError`.
//! - [`timestamp_ms`] (`pub(in crate::privileged_helper)`): `fn timestamp_ms() -> u64`.
//! - [`permission_help`] (`pub(super)`): `fn permission_help(operation: &str, error: impl `[`std::fmt::Display`]`) -> `[`String`].
//! - [`collect_process_observation`] (`pub(in crate::privileged_helper)`): `fn collect_process_observation( id: `[`ProcessId`]`, ) -> `[`Result`]`<`[`crate::ipc::acquisition::ObservationData`]`>`.
//! - [`collect_thread_observation`] (`pub(in crate::privileged_helper)`): `fn collect_thread_observation( id: `[`ProcessId`]`, ) -> `[`Result`]`<`[`crate::ipc::acquisition::ObservationData`]`>`.
//! - [`collect_memory_maps`] (`pub(in crate::privileged_helper)`): `fn collect_memory_maps(id: `[`ProcessId`]`) -> `[`Result`]`<`[`crate::ipc::acquisition::MemoryMaps`]`>`.
//! - [`read_environment`] (`pub(in crate::privileged_helper)`): `fn read_environment( id: `[`ProcessId`]`, ) -> `[`Result`]`<`[`crate::ipc::acquisition::EnvironmentData`]`>`.
//! - [`read_auxiliary_vector`] (`pub(in crate::privileged_helper)`): `fn read_auxiliary_vector( id: `[`ProcessId`]`, ) -> `[`Result`]`<`[`crate::ipc::acquisition::AuxData`]`>`.
//! - [`collect_file_descriptors`] (`pub(in crate::privileged_helper)`): `fn collect_file_descriptors( id: `[`ProcessId`]`, ) -> `[`Result`]`<`[`crate::ipc::descriptors::FileDescriptors`]`>`.
//! - [`memory_maps`] (`pub(in crate::privileged_helper)`): `fn memory_maps(pid: i32) -> `[`Result`]`<`[`Vec`]`<`[`MemoryMap`]`>>`.
//! - [`read_procfs_fields`] (`pub(in crate::privileged_helper)`): `fn read_procfs_fields( path: &str, ) -> `[`Result`]`<`[`std::collections::HashMap`]`<`[`String`]`, `[`String`]`>>`.
//! - [`socket_text`] (`pub(in crate::privileged_helper)`): `fn socket_text(path: &str) -> `[`Result`]`<`[`String`]`>`.
//! - [`inet_sockets`] (`pub(in crate::privileged_helper)`): `fn inet_sockets( text: &str, protocol: `[`crate::ipc::model::socket_types::SocketProtocol`]`, ) -> `[`std::collections::HashMap`]`<u64, `[`SocketInfo`]`>`.
//! - [`unix_sockets`] (`pub(in crate::privileged_helper)`): `fn unix_sockets( text: &str, ) -> `[`std::collections::HashMap`]`<u64, `[`SocketInfo`]`>`.
//! - [`unix_socket_peers`] (`pub(in crate::privileged_helper)`): `fn unix_socket_peers( namespace: &`[`super::NetworkNamespaceReader`]`, deadline: `[`std::time::Instant`]`, ) -> `[`Result`]`<`[`std::collections::HashMap`]`<u64, `[`SocketInfo`]`>>`.

use anyhow::Result;
use std::time::{SystemTime, UNIX_EPOCH};
use {
    super::{env, monitoring, procfs, validate_process_identity},
    crate::{
        ipc::{
            model::{memory::MemoryMap, sockets::SocketInfo},
            schema::ProcessId,
        },
        privileged_helper::process::{maps, sockets},
    },
};

pub(in crate::privileged_helper) fn timestamp_ms() -> u64 {
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
pub(in crate::privileged_helper) enum SubscribeError {
    Stopped,
    TooManySubscribers,
}

pub(in crate::privileged_helper) fn collect_process_observation(
    id: ProcessId,
) -> Result<crate::ipc::acquisition::ObservationData> {
    Ok(crate::ipc::acquisition::ObservationData {
        process_sample: monitoring::capture_sample(id)?,
        threads_only: false,
    })
}

pub(in crate::privileged_helper) fn collect_thread_observation(
    id: ProcessId,
) -> Result<crate::ipc::acquisition::ObservationData> {
    Ok(crate::ipc::acquisition::ObservationData {
        process_sample: monitoring::capture_sample(id)?,
        threads_only: true,
    })
}

use crate::ipc::acquisition::MemoryMaps;

pub(in crate::privileged_helper) fn collect_memory_maps(id: ProcessId) -> Result<MemoryMaps> {
    validate_process_identity(id)?;
    let result = maps::read_smaps(id.pid);
    let rollup = maps::read_memory_rollup(id.pid);
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

pub(in crate::privileged_helper) fn read_environment(
    id: ProcessId,
) -> Result<crate::ipc::acquisition::EnvironmentData> {
    env::read_environment(id)
}

pub(in crate::privileged_helper) fn read_auxiliary_vector(
    id: ProcessId,
) -> Result<crate::ipc::acquisition::AuxData> {
    super::auxv::read_auxiliary_vector(id)
}

pub(in crate::privileged_helper) fn collect_file_descriptors(
    id: ProcessId,
) -> Result<crate::ipc::descriptors::FileDescriptors> {
    super::fds::collect_file_descriptors(id)
}

// Minimal primitives also used by system-wide monitoring.
pub(in crate::privileged_helper) fn memory_maps(pid: i32) -> Result<Vec<MemoryMap>> {
    maps::read_maps(pid)
}

#[cfg(test)]
pub(in crate::privileged_helper) fn ticks_per_second() -> f64 {
    procfs::ticks_per_second()
}

pub(in crate::privileged_helper) fn read_procfs_fields(
    path: &str,
) -> Result<std::collections::HashMap<String, String>> {
    procfs::read_procfs_fields(path)
}

pub(in crate::privileged_helper) fn socket_text(path: &str) -> Result<String> {
    sockets::read_socket_table(path)
}

pub(in crate::privileged_helper) fn inet_sockets(
    text: &str,
    protocol: crate::ipc::model::socket_types::SocketProtocol,
) -> std::collections::HashMap<u64, SocketInfo> {
    sockets::parse_inet_socket_table(text, protocol)
}

pub(in crate::privileged_helper) fn unix_sockets(
    text: &str,
) -> std::collections::HashMap<u64, SocketInfo> {
    sockets::parse_unix_socket_table(text)
}

pub(in crate::privileged_helper) fn unix_socket_peers(
    namespace: &super::NetworkNamespaceReader,
    deadline: std::time::Instant,
) -> Result<std::collections::HashMap<u64, SocketInfo>> {
    sockets::collect_unix_socket_peers(namespace, deadline)
}
