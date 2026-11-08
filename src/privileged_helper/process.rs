//! Protected process acquisition for IPC and system monitoring.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`acquire_process_elf_ranges`] | `pub(super)` | `fn acquire_process_elf_ranges( id: `[`ProcessId`]`, mapping_start: u64, ranges: `[`Vec`]`<`[`crate::ipc::elf::FileRange`]`>, ) -> `[`anyhow::Result`]`<`[`crate::ipc::schema::MonitorResponse`]`>` |
//! | [`ObservationSession`] | `pub(super)` | `struct ObservationSession` |
//! | [`collect_processes`] | `pub(super)` | `fn collect_processes() -> `[`Result`](anyhow::Result)`<`[`Vec`]`<`[`ProcessSummary`](crate::ipc::acquisition::ProcessSummary)`>>` |
//! | [`validate_process_identity`] | `pub(super)` | `fn validate_process_identity(id: `[`ProcessId`]`) -> `[`Result`](anyhow::Result)`<()>` |
//! | [`enable_privacy`] | `pub(super)` | `fn enable_privacy()` |
//! | [`privacy_enabled`] | `pub(super)` | `fn privacy_enabled() -> bool` |
//! | [`ProcessMonitor`] | `pub(super)` | `struct ProcessMonitor` |
//! | [`SubscribeError`] | `pub(super)` | `enum SubscribeError` |
//! | [`collect_file_descriptors`] | `pub(super)` | `fn collect_file_descriptors( id: `[`ProcessId`]`, ) -> `[`Result`](anyhow::Result)`<`[`crate::ipc::descriptors::FileDescriptors`]`>` |
//! | [`collect_memory_maps`] | `pub(super)` | `fn collect_memory_maps(id: `[`ProcessId`]`) -> `[`Result`](anyhow::Result)`<`[`MemoryMaps`](crate::ipc::acquisition::MemoryMaps)`>` |
//! | [`collect_process_observation`] | `pub(super)` | `fn collect_process_observation( id: `[`ProcessId`]`, ) -> `[`Result`](anyhow::Result)`<`[`crate::ipc::acquisition::ObservationData`]`>` |
//! | [`collect_thread_observation`] | `pub(super)` | `fn collect_thread_observation( id: `[`ProcessId`]`, ) -> `[`Result`](anyhow::Result)`<`[`crate::ipc::acquisition::ObservationData`]`>` |
//! | [`inet_sockets`] | `pub(super)` | `fn inet_sockets( text: &str, protocol: `[`crate::ipc::model::socket_types::SocketProtocol`]`, ) -> `[`std::collections::HashMap`]`<u64, `[`SocketInfo`](crate::ipc::model::sockets::SocketInfo)`>` |
//! | [`memory_maps`] | `pub(super)` | `fn memory_maps(pid: i32) -> `[`Result`](anyhow::Result)`<`[`Vec`]`<`[`MemoryMap`](crate::ipc::model::memory::MemoryMap)`>>` |
//! | [`read_auxiliary_vector`] | `pub(super)` | `fn read_auxiliary_vector( id: `[`ProcessId`]`, ) -> `[`Result`](anyhow::Result)`<`[`crate::ipc::acquisition::AuxData`]`>` |
//! | [`read_environment`] | `pub(super)` | `fn read_environment( id: `[`ProcessId`]`, ) -> `[`Result`](anyhow::Result)`<`[`crate::ipc::acquisition::EnvironmentData`]`>` |
//! | [`read_procfs_fields`] | `pub(super)` | `fn read_procfs_fields( path: &str, ) -> `[`Result`](anyhow::Result)`<`[`std::collections::HashMap`]`<`[`String`]`, `[`String`]`>>` |
//! | [`socket_text`] | `pub(super)` | `fn socket_text(path: &str) -> `[`Result`](anyhow::Result)`<`[`String`]`>` |
//! | [`timestamp_ms`] | `pub(super)` | `fn timestamp_ms() -> u64` |
//! | [`unix_socket_peers`] | `pub(super)` | `fn unix_socket_peers( namespace: &`[`super::NetworkNamespace`](socket_namespace::NetworkNamespace)`, deadline: `[`std::time::Instant`]`, ) -> `[`Result`](anyhow::Result)`<`[`std::collections::HashMap`]`<u64, `[`SocketInfo`](crate::ipc::model::sockets::SocketInfo)`>>` |
//! | [`unix_sockets`] | `pub(super)` | `fn unix_sockets( text: &str, ) -> `[`std::collections::HashMap`]`<u64, `[`SocketInfo`](crate::ipc::model::sockets::SocketInfo)`>` |
//! | [`SocketMetadata`] | `pub(super)` | `struct SocketMetadata` |
//! | [`additional_sockets`] | `pub(super)` | `fn additional_sockets(pid: i32) -> `[`HashMap`](std::collections::HashMap)`<u64, `[`SocketInfo`](crate::ipc::model::sockets::SocketInfo)`>` |
//! | [`NetworkNamespace`] | `pub(super)` | `struct NetworkNamespace` |
//! | [`open_network_namespace`] | `pub(super)` | `fn open_network_namespace( id: `[`ProcessId`]`, ) -> `[`Result`](anyhow::Result)`<`[`NetworkNamespace`]`>` |
//! | [`create_process_monitor`] | `pub(in crate::privileged_helper)` | `fn create_process_monitor(interval: `[`Duration`](std::time::Duration)`) -> `[`ProcessMonitor`] |
//! | [`join_process_collectors`] | `pub(in crate::privileged_helper)` | `fn join_process_collectors( state: &`[`ProcessMonitor`]`, ) -> `[`Result`](anyhow::Result)`<()>` |
//! | [`observe_process_monitor`] | `pub(in crate::privileged_helper)` | `fn observe_process_monitor( monitor: &`[`Arc`](std::sync::Arc)`<`[`ProcessMonitor`]`>, id: `[`ProcessId`]`, permit: `[`ObservationPermit`](monitoring::service::ObservationPermit)`, ) -> `[`Result`](anyhow::Result)`<`[`ObservationSession`]`>` |
//! | [`process_monitor_is_stopped`] | `pub(in crate::privileged_helper)` | `fn process_monitor_is_stopped(state: &`[`ProcessMonitor`]`) -> bool` |
//! | [`reserve_process_monitor`] | `pub(in crate::privileged_helper)` | `fn reserve_process_monitor( monitor: &`[`Arc`](std::sync::Arc)`<`[`ProcessMonitor`]`>, ) -> `[`Result`](anyhow::Result)`<`[`ObservationPermit`](monitoring::service::ObservationPermit)`, `[`SubscribeError`]`>` |
//! | [`stop_process_monitor`] | `pub(in crate::privileged_helper)` | `fn stop_process_monitor(state: &`[`ProcessMonitor`]`)` |
//! | [`create_socket_metadata_reader`] | `pub(in crate::privileged_helper)` | `fn create_socket_metadata_reader( process_id: `[`ProcessId`]`, ) -> `[`SocketMetadataReader`](socket_metadata::SocketMetadataReader) |
//! | [`read_socket_metadata`] | `pub(in crate::privileged_helper)` | `fn read_socket_metadata( state: &mut `[`SocketMetadataReader`](socket_metadata::SocketMetadataReader)`, fd: u32, device: u64, inode: u64, ) -> `[`Result`](anyhow::Result)`<`[`SocketMetadata`]`>` |
//! | [`socket_metadata_has_observation`] | `pub(in crate::privileged_helper)` | `fn socket_metadata_has_observation( state: &`[`SocketMetadata`]`, ) -> bool` |
//!
//! Mapped ELF byte replies use [`MappedFileData`](crate::ipc::elf::MappedFileData).

pub(super) use monitoring::{ObservationSession, ProcessMonitor};
mod auxv;
mod discovery;
mod elf;
mod env;
mod fds;
mod identity;
mod maps;
mod memory;
mod monitoring;
mod procfs;
mod resources;
mod snapshot;
mod socket_metadata;
mod socket_namespace;
mod sockets;
mod threads;
use crate::ipc::schema::ProcessId;
pub(super) use discovery::collect_processes;
use discovery::{collect_process_summary, read_user_names};
#[cfg(test)]
pub(super) use identity::read_process_identity;
pub(super) use identity::validate_process_identity;
pub(super) use memory::{enable_privacy, privacy_enabled};

use resources::permission_help;
#[cfg(test)]
pub(super) use resources::ticks_per_second;
pub(super) use resources::{
    SubscribeError, collect_file_descriptors, collect_memory_maps, collect_process_observation,
    collect_thread_observation, inet_sockets, memory_maps, read_auxiliary_vector, read_environment,
    read_procfs_fields, socket_text, timestamp_ms, unix_socket_peers, unix_sockets,
};
pub(super) use socket_metadata::{SocketMetadata, additional_sockets};
pub(super) use socket_namespace::{NetworkNamespace, open_network_namespace};
#[cfg(test)]
mod test_support;
#[cfg(test)]
pub(super) use socket_metadata::require_socket_metadata;
#[cfg(test)]
pub(super) use test_support::Target;
#[cfg(test)]
mod tests;

/// Acquire only a currently executable mapping selected by its start address.
pub(super) fn acquire_process_elf_ranges(
    id: ProcessId,
    mapping_start: u64,
    ranges: Vec<crate::ipc::elf::FileRange>,
) -> anyhow::Result<crate::ipc::schema::MonitorResponse> {
    validate_process_identity(id)?;
    let maps = maps::read_maps(id.pid)?;
    let result = match maps
        .iter()
        .find(|map| map.start == mapping_start && map.executable && map.inode != 0)
    {
        Some(map) => elf::read_mapped_file_ranges(id.pid, map, &ranges),
        None => Ok(crate::ipc::schema::MonitorResponse::Elf(None)),
    };
    validate_process_identity(id)?;
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
