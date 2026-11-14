//! Protected process acquisition for IPC and system monitoring.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`acquire_process_elf_ranges`] | `pub(super)` | `fn acquire_process_elf_ranges( id: `[`ProcessId`]`, mapping_start: u64, ranges: `[`Vec`]`<`[`crate::ipc::elf::FileRange`]`>, ) -> `[`anyhow::Result`]`<`[`crate::ipc::schema::MonitorResponse`]`>` |
//! | [`ObservationSession`] | `pub(super)` | `struct ObservationSession` |
//! | [`ProcessMonitor`] | `pub(super)` | `struct ProcessMonitor` |
//! | [`collect_processes`] | `pub(super)` | `fn collect_processes() -> `[`Result`](anyhow::Result)`<`[`Vec`]`<`[`ProcessSummary`](crate::ipc::acquisition::ProcessSummary)`>>` |
//! | [`validate_process_id`] | `pub(super)` | `fn validate_process_id(id: `[`ProcessId`]`) -> `[`Result`](anyhow::Result)`<()>` |
//! | [`enable_privacy`] | `pub(super)` | `fn enable_privacy()` |
//! | [`privacy_enabled`] | `pub(super)` | `fn privacy_enabled() -> bool` |
//! | [`SubscribeError`] | `pub(super)` | `enum SubscribeError` |
//! | [`collect_file_descriptors`] | `pub(super)` | `fn collect_file_descriptors( id: `[`ProcessId`]`, ) -> `[`Result`](anyhow::Result)`<`[`crate::ipc::descriptors::FileDescriptors`]`>` |
//! | [`collect_memory_maps`] | `pub(super)` | `fn collect_memory_maps(id: `[`ProcessId`]`) -> `[`Result`](anyhow::Result)`<`[`MemoryMaps`](crate::ipc::acquisition::MemoryMaps)`>` |
//! | [`collect_process_observation`] | `pub(super)` | `fn collect_process_observation( id: `[`ProcessId`]`, ) -> `[`Result`](anyhow::Result)`<`[`crate::ipc::acquisition::ObservationData`]`>` |
//! | [`collect_thread_observation`] | `pub(super)` | `fn collect_thread_observation( id: `[`ProcessId`]`, ) -> `[`Result`](anyhow::Result)`<`[`crate::ipc::acquisition::ObservationData`]`>` |
//! | [`parse_inet_socket_table`] | `pub(super)` | `fn parse_inet_socket_table( text: &str, protocol: `[`crate::ipc::model::socket_types::SocketProtocol`]`, ) -> `[`std::collections::HashMap`]`<u64, `[`SocketInfo`](crate::ipc::model::sockets::SocketInfo)`>` |
//! | [`read_maps`] | `pub(super)` | `fn read_maps(id: `[`ProcessId`]`) -> `[`Result`](anyhow::Result)`<`[`Vec`]`<`[`MemoryMap`](crate::ipc::model::memory::MemoryMap)`>>` |
//! | [`read_auxiliary_vector`] | `pub(super)` | `fn read_auxiliary_vector( id: `[`ProcessId`]`, ) -> `[`Result`](anyhow::Result)`<`[`crate::ipc::acquisition::AuxData`]`>` |
//! | [`read_environment`] | `pub(super)` | `fn read_environment( id: `[`ProcessId`]`, ) -> `[`Result`](anyhow::Result)`<`[`crate::ipc::acquisition::EnvironmentData`]`>` |
//! | [`read_procfs_fields`] | `pub(super)` | `fn read_procfs_fields( path: &str, ) -> `[`Result`](anyhow::Result)`<`[`std::collections::HashMap`]`<`[`String`]`, `[`String`]`>>` |
//! | [`read_socket_table`] | `pub(super)` | `fn read_socket_table(path: &str) -> `[`Result`](anyhow::Result)`<`[`String`]`>` |
//! | [`collect_unix_socket_peers`] | `pub(super)` | `fn collect_unix_socket_peers( namespace: &`[`NetworkNamespace`](socket_namespace::NetworkNamespace)`, deadline: `[`std::time::Instant`]`, ) -> `[`Result`](anyhow::Result)`<`[`std::collections::HashMap`]`<u64, `[`SocketInfo`](crate::ipc::model::sockets::SocketInfo)`>>` |
//! | [`parse_unix_socket_table`] | `pub(super)` | `fn parse_unix_socket_table( text: &str, ) -> `[`std::collections::HashMap`]`<u64, `[`SocketInfo`](crate::ipc::model::sockets::SocketInfo)`>` |
//! | [`SocketMetadata`] | `pub(super)` | `struct SocketMetadata` |
//! | [`additional_sockets`] | `pub(super)` | `fn additional_sockets(id: `[`ProcessId`]`) -> `[`HashMap`](std::collections::HashMap)`<u64, `[`SocketInfo`](crate::ipc::model::sockets::SocketInfo)`>` |
//! | [`open_network_namespace`] | `pub(super)` | `fn open_network_namespace( id: `[`ProcessId`]`, ) -> `[`Result`](anyhow::Result)`<`[`NetworkNamespace`](socket_namespace::NetworkNamespace)`>` |
//! | [`create_process_monitor`] | `pub(in crate::privileged_helper)` | `fn create_process_monitor(interval: `[`Duration`](std::time::Duration)`) -> `[`ProcessMonitor`] |
//! | [`join_process_collectors`] | `pub(in crate::privileged_helper)` | `fn join_process_collectors(state: &`[`ProcessMonitor`]`) -> `[`Result`](anyhow::Result)`<()>` |
//! | [`observe_process_monitor`] | `pub(in crate::privileged_helper)` | `fn observe_process_monitor( monitor: &`[`Arc`](std::sync::Arc)`<`[`ProcessMonitor`]`>, id: `[`ProcessId`]`, permit: `[`ObservationPermit`](monitoring::service::ObservationPermit)`, ) -> `[`Result`](anyhow::Result)`<`[`ObservationSession`]`>` |
//! | [`process_monitor_is_stopped`] | `pub(in crate::privileged_helper)` | `fn process_monitor_is_stopped(state: &`[`ProcessMonitor`]`) -> bool` |
//! | [`reserve_process_monitor`] | `pub(in crate::privileged_helper)` | `fn reserve_process_monitor( monitor: &`[`Arc`](std::sync::Arc)`<`[`ProcessMonitor`]`>, ) -> `[`Result`](anyhow::Result)`<`[`ObservationPermit`](monitoring::service::ObservationPermit)`, `[`SubscribeError`]`>` |
//! | [`stop_process_monitor`] | `pub(in crate::privileged_helper)` | `fn stop_process_monitor(state: &`[`ProcessMonitor`]`)` |
//! | [`create_socket_metadata_reader`] | `pub(in crate::privileged_helper)` | `fn create_socket_metadata_reader( process_id: `[`ProcessId`]`, ) -> `[`SocketMetadataReader`](socket_metadata::SocketMetadataReader) |
//! | [`read_socket_metadata`] | `pub(in crate::privileged_helper)` | `fn read_socket_metadata( reader: &mut `[`SocketMetadataReader`](socket_metadata::SocketMetadataReader)`, fd: u32, device: u64, inode: u64, ) -> `[`Result`](anyhow::Result)`<`[`SocketMetadata`]`>` |
//! | [`socket_metadata_has_observation`] | `pub(in crate::privileged_helper)` | `fn socket_metadata_has_observation( state: &`[`SocketMetadata`]`, ) -> bool` |
//!
//! Mapped ELF byte replies use [`MappedFileData`](crate::ipc::elf::MappedFileData).

pub(super) use monitoring::{ObservationSession, ProcessMonitor};
mod auxv;
mod collect_process;
mod elf;
mod env;
mod fds;
mod maps;
mod memory;
mod monitoring;
mod procfs;
mod snapshot;
mod socket_metadata;
mod socket_namespace;
mod sockets;
mod threads;
mod validate_process_id;
use crate::ipc::schema::ProcessId;
use collect_process::collect_process_summary;
pub(super) use collect_process::collect_processes;
pub(super) use memory::{enable_privacy, privacy_enabled};
#[cfg(test)]
pub(super) use validate_process_id::read_process_id;
pub(super) use validate_process_id::validate_process_id;

pub(super) use auxv::read_auxiliary_vector;
pub(super) use env::read_environment;
pub(super) use fds::collect_file_descriptors;
pub(super) use maps::{collect_memory_maps, read_maps};
pub(super) use monitoring::{
    SubscribeError, collect_process_observation, collect_thread_observation,
};
pub(super) use procfs::read_procfs_fields;
#[cfg(test)]
pub(super) use procfs::ticks_per_second;
pub(super) use socket_metadata::{SocketMetadata, additional_sockets};
pub(super) use socket_namespace::open_network_namespace;
pub(super) use sockets::{
    collect_unix_socket_peers, parse_inet_socket_table, parse_unix_socket_table, read_socket_table,
};
#[cfg(test)]
mod test_support;
#[cfg(test)]
pub(super) use socket_metadata::require_socket_metadata;
#[cfg(test)]
use test_support::Target;
#[cfg(test)]
mod tests;

/// Acquire only a currently executable mapping selected by its start address.
pub(super) fn acquire_process_elf_ranges(
    id: ProcessId,
    mapping_start: u64,
    ranges: Vec<crate::ipc::elf::FileRange>,
) -> anyhow::Result<crate::ipc::schema::MonitorResponse> {
    validate_process_id(id)?;
    let maps = maps::read_maps(id)?;
    let result = match maps
        .iter()
        .find(|map| map.start == mapping_start && map.executable && map.inode != 0)
    {
        Some(map) => elf::read_mapped_file_ranges(id, map, &ranges),
        None => Ok(crate::ipc::schema::MonitorResponse::Elf(None)),
    };
    validate_process_id(id)?;
    result
}

pub(in crate::privileged_helper) use monitoring::{
    create_process_monitor, join_process_collectors, observe_process_monitor,
    process_monitor_is_stopped, reserve_process_monitor, stop_process_monitor,
};

pub(in crate::privileged_helper) use socket_metadata::{
    create_socket_metadata_reader, read_socket_metadata, socket_metadata_has_observation,
};

#[cfg(test)]
pub(in crate::privileged_helper) use monitoring::observer_count_process_monitor;

#[cfg(test)]
pub(in crate::privileged_helper) use test_support::create_network_namespace_target;

#[cfg(test)]
pub(in crate::privileged_helper) use test_support::create_target;

fn permission_help(operation: &str, error: impl std::fmt::Display) -> String {
    format!(
        "{operation}: {error}. If access is denied, check the process owner, kernel.yama.ptrace_scope, and CAP_SYS_PTRACE. sudo is never run automatically."
    )
}
