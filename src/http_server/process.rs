//! Process façade for HTTP transport and system monitoring.
//!
//! # Interface
//!
//! ## Types
//!
//! | Definition | Visibility | Kind / type |
//! | --- | --- | --- |
//! | [`ProcessScanner`] | `pub(super)` | `struct `[`ProcessScanner`] |
//! | [`ProcessSummary`] | `pub(super)` | `struct `[`ProcessSummary`] |
//! | [`ProcessId`] | `pub(super)` | `struct `[`ProcessId`] |
//! | [`MemoryMap`] | `pub(super)` | `struct `[`MemoryMap`] |
//! | [`ProcessMonitor`] | `pub(super)` | `struct `[`ProcessMonitor`] |
//! | [`SubscribeError`] | `pub(super)` | `enum `[`SubscribeError`] |
//! | [`SocketInfo`] | `pub(super)` | `struct `[`SocketInfo`] |
//!
//! ## Functions
//!
//! | Definition | Visibility | Signature |
//! | --- | --- | --- |
//! | [`check_identity`] | `pub(super)` | `fn check_identity(id: `[`ProcessId`]`) -> `[`Result`](anyhow::Result)`<()>` |
//! | [`observation`] | `pub(super)` | `fn observation(id: `[`ProcessId`]`) -> `[`Result`](anyhow::Result)`<impl `[`Serialize`](serde::Serialize)`>` |
//! | [`fn@threads`] | `pub(super)` | `fn threads(id: `[`ProcessId`]`) -> `[`Result`](anyhow::Result)`<impl `[`Serialize`](serde::Serialize)`>` |
//! | [`fn@maps`] | `pub(super)` | `fn maps(id: `[`ProcessId`]`) -> `[`Result`](anyhow::Result)`<impl `[`Serialize`](serde::Serialize)`>` |
//! | [`environment`] | `pub(super)` | `fn environment(id: `[`ProcessId`]`) -> `[`Result`](anyhow::Result)`<impl `[`Serialize`](serde::Serialize)`>` |
//! | [`auxv`] | `pub(super)` | `fn auxv(id: `[`ProcessId`]`) -> `[`Result`](anyhow::Result)`<impl `[`Serialize`](serde::Serialize)`>` |
//! | [`fn@fds`] | `pub(super)` | `fn fds(id: `[`ProcessId`]`) -> `[`Result`](anyhow::Result)`<impl `[`Serialize`](serde::Serialize)`>` |
//! | [`memory_maps`] | `pub(super)` | `fn memory_maps(pid: `[`i32`]`) -> `[`Result`](anyhow::Result)`<`[`Vec`]`<`[`MemoryMap`]`>>` |
//! | [`ticks_per_second`] | `pub(super)` | `fn ticks_per_second() -> `[`f64`] |
//! | [`fields`] | `pub(super)` | `fn fields(path: &`[`str`]`) -> `[`Result`](anyhow::Result)`<`[`HashMap`](std::collections::HashMap)`<`[`String`]`, `[`String`]`>>` |
//! | [`socket_text`] | `pub(super)` | `fn socket_text(path: &`[`str`]`) -> `[`Result`](anyhow::Result)`<`[`String`]`>` |
//! | [`inet_sockets`] | `pub(super)` | `fn inet_sockets(text: &`[`str`]`, protocol: `[`super::socket_types::SocketProtocol`]`) -> `[`HashMap`](std::collections::HashMap)`<`[`u64`]`, `[`SocketInfo`]`>` |
//! | [`unix_sockets`] | `pub(super)` | `fn unix_sockets(text: &`[`str`]`) -> `[`HashMap`](std::collections::HashMap)`<`[`u64`]`, `[`SocketInfo`]`>` |
//! | [`unix_socket_peers`] | `pub(super)` | `fn unix_socket_peers(deadline: `[`Instant`](std::time::Instant)`) -> `[`Result`](anyhow::Result)`<`[`HashMap`](std::collections::HashMap)`<`[`u64`]`, `[`SocketInfo`]`>>` |
//! | [`timestamp_ms`] | `pub(super)` | `fn timestamp_ms() -> `[`u64`] |
//!
mod details;
mod discovery;
mod fds;
mod identity;
mod maps;
mod memory;
mod monitoring;
mod procfs;
mod resources;
mod snapshot;
mod sockets;
mod threads;
pub(super) use discovery::{ProcessScanner, ProcessSummary};
use discovery::{summary, users};
#[cfg(test)]
pub(super) use identity::identity;
pub(super) use identity::{ProcessId, check_identity};
pub(super) use maps::MemoryMap;
pub(super) use monitoring::ProcessMonitor;
use resources::permission_help;
pub(super) use resources::{
    SubscribeError, auxv, environment, fds, fields, inet_sockets, maps, memory_maps, observation,
    socket_text, threads, ticks_per_second, timestamp_ms, unix_socket_peers, unix_sockets,
};
pub(super) use sockets::SocketInfo;
#[cfg(test)]
mod test_support;
#[cfg(test)]
pub(super) use test_support::Target as TestTarget;
#[cfg(test)]
mod tests;
