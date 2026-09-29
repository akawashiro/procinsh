//! Process façade for HTTP transport and system monitoring.
//!
//! # Interface
//!
//! Re-export visibility: `pub(super)`. Names link to definitions; **Source** opens their implementation.
//! `Result<T>` below means [`anyhow::Result<T>`](anyhow::Result).
//!
//! ## Types and constants
//!
//! | Definition | Kind / type |
//! | --- | --- |
//! | [`Discovery`] | `struct Discovery` |
//! | [`ProcessSummary`] | `struct ProcessSummary` |
//! | [`ProcessId`] | `struct ProcessId` |
//! | [`MemoryMap`] | `struct MemoryMap` |
//! | [`Monitoring`] | `struct Monitoring` |
//! | [`Snapshotter`] | `struct Snapshotter` |
//! | [`SubscribeError`] | `enum SubscribeError` |
//! | [`SocketInfo`] | `struct SocketInfo` |
//! | [`MAX_READ`] | `const MAX_READ: usize` |
//!
//! ## Functions
//!
//! | Definition | Signature |
//! | --- | --- |
//! | [`check_identity`] | `fn check_identity(id: ProcessId) -> Result<()>` |
//! | [`observation`] | `fn observation(id: ProcessId) -> Result<impl Serialize>` |
//! | [`fn@threads`] | `fn threads(id: ProcessId) -> Result<impl Serialize>` |
//! | [`fn@maps`] | `fn maps(id: ProcessId) -> Result<impl Serialize>` |
//! | [`environment`] | `fn environment(id: ProcessId) -> Result<impl Serialize>` |
//! | [`auxv`] | `fn auxv(id: ProcessId) -> Result<impl Serialize>` |
//! | [`fn@fds`] | `fn fds(id: ProcessId) -> Result<impl Serialize>` |
//! | [`fn@signals`] | `fn signals(id: ProcessId) -> Result<impl Serialize>` |
//! | [`fn@memory`] | `fn memory(id: ProcessId, address: u64, length: usize) -> Result<impl Serialize>` |
//! | [`memory_maps`] | `fn memory_maps(pid: i32) -> Result<Vec<MemoryMap>>` |
//! | [`ticks_per_second`] | `fn ticks_per_second() -> f64` |
//! | [`fields`] | `fn fields(path: &str) -> Result<HashMap<String, String>>` |
//! | [`socket_text`] | `fn socket_text(path: &str) -> Result<String>` |
//! | [`inet_sockets`] | `fn inet_sockets(text: &str, protocol: &str) -> HashMap<u64, SocketInfo>` |
//! | [`unix_sockets`] | `fn unix_sockets(text: &str) -> HashMap<u64, SocketInfo>` |
//! | [`unix_socket_peers`] | `fn unix_socket_peers(deadline: Instant) -> Result<HashMap<u64, SocketInfo>>` |
//! | [`timestamp_ms`] | `fn timestamp_ms() -> u64` |
//!
//! Supporting types: [`Serialize`](serde::Serialize), [`HashMap`](std::collections::HashMap),
//! [`Instant`](std::time::Instant). Test-only re-exports are omitted.
mod details;
mod discovery;
mod fds;
mod identity;
mod maps;
mod memory;
mod monitoring;
mod procfs;
mod resources;
mod signals;
mod snapshot;
mod sockets;
mod threads;
pub(super) use discovery::{Discovery, ProcessSummary};
use discovery::{summary, users};
#[cfg(test)]
pub(super) use identity::identity;
pub(super) use identity::{ProcessId, check_identity};
pub(super) use maps::MemoryMap;
pub(super) use monitoring::Monitoring;
use resources::permission_help;
pub(super) use resources::{
    MAX_READ, Snapshotter, SubscribeError, auxv, environment, fds, fields, inet_sockets, maps,
    memory, memory_maps, observation, signals, socket_text, threads, ticks_per_second,
    timestamp_ms, unix_socket_peers, unix_sockets,
};
pub(super) use sockets::SocketInfo;
#[cfg(test)]
mod test_support;
#[cfg(test)]
pub(super) use test_support::Target as TestTarget;
#[cfg(test)]
mod tests;
