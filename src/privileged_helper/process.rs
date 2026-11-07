//! Protected process acquisition for IPC and system monitoring.
//!
//! # Interface
//!
//! ## Types
//!
//! | Definition | Visibility | Kind / type |
//! | --- | --- | --- |
//! | [`ProcessSummary`] | `pub(super)` | `struct `[`ProcessSummary`] |
//! | [`ProcessId`] | `pub(super)` | `struct `[`ProcessId`] |
//! | [`MemoryMap`] | `pub(super)` | `struct `[`MemoryMap`] |
//! | [`ObservationSession`] | `pub(super)` | `struct ObservationSession` |
//! | [`ProcessMonitor`] | `pub(super)` | `struct `[`ProcessMonitor`] |
//! | [`SubscribeError`] | `pub(super)` | `enum `[`SubscribeError`] |
//! | [`SocketInfo`] | `pub(super)` | `struct `[`SocketInfo`] |
//! | [`SocketMetadataReader`] | `pub(super)` | `struct `[`SocketMetadataReader`] |
//! | [`NetworkNamespaceReader`] | `pub(super)` | `struct `[`NetworkNamespaceReader`] |
//! | [`SocketMetadata`] | `pub(super)` | `struct `[`SocketMetadata`] |
//!
//! ## Functions
//!
//! | Definition | Visibility | Signature |
//! | --- | --- | --- |
//! | [`collect_processes`] | `pub(super)` | `fn collect_processes() -> anyhow::Result<Vec<ProcessSummary>>` |
//! | [`validate_process_identity`] | `pub(super)` | `fn validate_process_identity(id: `[`ProcessId`]`) -> `[`Result`](anyhow::Result)`<()>` |
//! | [`collect_process_observation`] | `pub(super)` | `fn collect_process_observation (id: ProcessId) -> anyhow::Result<`[`ObservationData`](crate::ipc::acquisition::ObservationData)`>` |
//! | [`collect_thread_observation`] | `pub(super)` | `fn collect_thread_observation (id: ProcessId) -> anyhow::Result<`[`ObservationData`](crate::ipc::acquisition::ObservationData)`>` |
//! | [`collect_memory_maps`] | `pub(super)` | `fn collect_memory_maps (id: ProcessId) -> anyhow::Result<`[`MemoryMaps`](crate::ipc::acquisition::MemoryMaps)`>` |
//! | [`read_environment`] | `pub(super)` | `fn read_environment (id: ProcessId) -> anyhow::Result<`[`EnvironmentData`](crate::ipc::acquisition::EnvironmentData)`>` |
//! | [`read_auxiliary_vector`] | `pub(super)` | `fn read_auxiliary_vector (id: ProcessId) -> anyhow::Result<`[`AuxData`](crate::ipc::acquisition::AuxData)`>` |
//! | [`collect_file_descriptors`] | `pub(super)` | `fn collect_file_descriptors (id: ProcessId) -> anyhow::Result<`[`FileDescriptors`](crate::ipc::descriptors::FileDescriptors)`>` |
//! | [`memory_maps`] | `pub(super)` | `fn memory_maps(pid: `[`i32`]`) -> `[`Result`](anyhow::Result)`<`[`Vec`]`<`[`MemoryMap`]`>>` |
//! | [`read_procfs_fields`] | `pub(super)` | `fn read_procfs_fields(path: &`[`str`]`) -> `[`Result`](anyhow::Result)`<`[`HashMap`](std::collections::HashMap)`<`[`String`]`, `[`String`]`>>` |
//! | [`socket_text`] | `pub(super)` | `fn socket_text(path: &`[`str`]`) -> `[`Result`](anyhow::Result)`<`[`String`]`>` |
//! | [`inet_sockets`] | `pub(super)` | `fn inet_sockets(text: &`[`str`]`, protocol: `[`super::socket_types::SocketProtocol`]`) -> `[`HashMap`](std::collections::HashMap)`<`[`u64`]`, `[`SocketInfo`]`>` |
//! | [`unix_sockets`] | `pub(super)` | `fn unix_sockets(text: &`[`str`]`) -> `[`HashMap`](std::collections::HashMap)`<`[`u64`]`, `[`SocketInfo`]`>` |
//! | [`unix_socket_peers`] | `pub(super)` | `fn unix_socket_peers(namespace: &`[`NetworkNamespaceReader`]`, deadline: `[`Instant`](std::time::Instant)`) -> `[`Result`](anyhow::Result)`<`[`HashMap`](std::collections::HashMap)`<`[`u64`]`, `[`SocketInfo`]`>>` |
//! | [`additional_sockets`] | `pub(super)` | `fn additional_sockets(pid: `[`i32`]`) -> `[`HashMap`](std::collections::HashMap)`<`[`u64`]`, `[`SocketInfo`]`>` |
//! | [`acquire_process_elf_ranges`] | `pub(super)` | `fn acquire_process_elf_ranges(id: ProcessId, mapping_start: u64, ranges: Vec<crate::ipc::elf::FileRange>) -> anyhow::Result<crate::ipc::schema::MonitorResponse>` |
//! | [`timestamp_ms`] | `pub(super)` | `fn timestamp_ms() -> `[`u64`] |
//! | [`enable_privacy`] | `pub(super)` | `fn enable_privacy()` |
//! | [`privacy_enabled`] | `pub(super)` | `fn privacy_enabled() -> `[`bool`] |
//!
//! Mapped ELF byte replies use [`MappedFileData`](crate::ipc::elf::MappedFileData).
pub(super) use monitoring::ObservationSession;
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
pub(super) use discovery::{ProcessSummary, collect_processes};
use discovery::{collect_process_summary, read_user_names};
#[cfg(test)]
pub(super) use identity::read_process_identity;
pub(super) use identity::{ProcessId, validate_process_identity};
pub(super) use maps::MemoryMap;
pub(super) use memory::{enable_privacy, privacy_enabled};
pub(super) use monitoring::ProcessMonitor;
use resources::permission_help;
#[cfg(test)]
pub(super) use resources::ticks_per_second;
pub(super) use resources::{
    SubscribeError, collect_file_descriptors, collect_memory_maps, collect_process_observation,
    collect_thread_observation, inet_sockets, memory_maps, read_auxiliary_vector, read_environment,
    read_procfs_fields, socket_text, timestamp_ms, unix_socket_peers, unix_sockets,
};
pub(super) use socket_metadata::{SocketMetadata, SocketMetadataReader, additional_sockets};
pub(super) use socket_namespace::NetworkNamespaceReader;
pub(super) use sockets::SocketInfo;
#[cfg(test)]
mod test_support;
#[cfg(test)]
pub(super) use socket_metadata::require_socket_metadata;
#[cfg(test)]
pub(super) use test_support::NetworkNamespaceTarget;
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
