//! Process façade for HTTP transport and system monitoring.
//!
//! # Interface
//!
//! ## Types and constants
//!
//! | Definition | Visibility | Kind / type |
//! | --- | --- | --- |
//! | [`Discovery`] | `pub(super)` | `struct `[`Discovery`] |
//! | [`ProcessSummary`] | `pub(super)` | `struct `[`ProcessSummary`] |
//! | [`ProcessId`] | `pub(super)` | `struct `[`ProcessId`] |
//! | [`MemoryMap`] | `pub(super)` | `struct `[`MemoryMap`] |
//! | [`Monitoring`] | `pub(super)` | `struct `[`Monitoring`] |
//! | [`SubscribeError`] | `pub(super)` | `enum `[`SubscribeError`] |
//! | [`SocketInfo`] | `pub(super)` | `struct `[`SocketInfo`] |
//!
//! [`maps::MemoryKind`] is `pub(in crate::http_server) enum MemoryKind { Integer, Stack, Heap, SharedLibrary, Executable, File, Anonymous }`.
//! [`maps::MemoryMap::kind`] is `pub(super) fn kind(&self) -> MemoryKind`.
//! [`MemoryMap`] exposes the existing `readable`, `writable`, `executable`, `private` booleans; the redundant `permissions` string is removed.
//! [`MemoryMap`] uses `device: DeviceId` ([`super::resource::DeviceId`]) and `inode: u64` serialized as a decimal string.
//!
//! [`SocketInfo`] fields use [`super::socket_types::SocketProtocol`] and [`super::socket_types::SocketState`];
//! `local` and `remote` remain `Option<std::net::SocketAddr>` internally.
//! [`fds::Descriptor`] uses `kind: FdKind, access: FdAccess, protocol: Option<SocketProtocol>, state: Option<SocketState>, local: Option<InetAddress>, remote: Option<InetAddress>, path: Option<String>`;
//! [`fds::Endpoint::access`] is `pub(super) access: FdAccess`.
//!
//! Structured thread/signal payloads (types and fields `pub(super)`):
//! - [`threads::SchedulerPolicy`]: `Other, Fifo, Rr, Batch, Idle, Deadline, Ext, Unknown(u32)`.
//! - [`threads::CpuRange`]: `start: u32, end: u32` (inclusive).
//! - [`threads::ThreadSample`]: raw thread metadata and counters, including `ticks: u64, start_time: u64`; no CPU rate.
//! - [`threads::read`]: `pub(super) fn read(pid: i32, tid: i32) -> anyhow::Result<ThreadSample>`.
//! - [`threads::ThreadSample::observation`]: `pub(super) fn observation(&self) -> ThreadObservation`.
//! - [`threads::ThreadObservation`]: `scheduler: SchedulerPolicy, affinity: Option<Vec<CpuRange>>`.
//! - [`signals::Signal`]: `number: u32, name: String`.
//! - [`signals::SignalQueue`]: `count: u64, limit: u64` (decimal strings in JSON).
//! - [`signals::Mask`]: `bits: u64` (serialized as `hex`), `signals: Vec<Signal>`.
//! - [`signals::SignalStatus::queued`]: `pub(super) queued: SignalQueue`.
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
//! | [`fn@signals`] | `pub(super)` | `fn signals(id: `[`ProcessId`]`) -> `[`Result`](anyhow::Result)`<impl `[`Serialize`](serde::Serialize)`>` |
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
    SubscribeError, auxv, environment, fds, fields, inet_sockets, maps, memory_maps, observation,
    signals, socket_text, threads, ticks_per_second, timestamp_ms, unix_socket_peers, unix_sockets,
};
pub(super) use sockets::SocketInfo;
#[cfg(test)]
mod test_support;
#[cfg(test)]
pub(super) use test_support::Target as TestTarget;
#[cfg(test)]
mod tests;
