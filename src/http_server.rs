//! HTTP server wiring and shared API.
//!
//! # Interface
//!
//! | Definition | Visibility | Signature |
//! | --- | --- | --- |
//! | [`run`] | `pub(super)` | `async fn run(listen: `[`SocketAddr`](std::net::SocketAddr)`, interval: `[`Duration`](std::time::Duration)`) -> `[`anyhow::Result`]`<()>` |
//!
//! Shared identities (all `pub(super)`, fields also `pub(super)`):
//! - [`resource::DeviceId`]: `major: u32, minor: u32`;
//!   [`resource::DeviceId::from_stat`]: `fn from_stat(device: u64) -> Self`;
//!   [`resource::DeviceId::from_kernel`]: `fn from_kernel(device: u64) -> Self`.
//! - [`resource::FileIdentity`]: `device: DeviceId, inode: u64, generation: u32`.
//! - [`resource::IpcIdentity`]: `kind: IpcKind, device: DeviceId, inode: u64`.
//! - [`resource::IpcKind`]: `Pipe, Socket`.
//! - [`resource::decimal`]: `fn decimal<S: serde::Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>`.
//! Inodes serialize as decimal strings.
//!
//! Shared state: [`state::AppState::system_monitor`] is
//! `pub(super) system_monitor: Arc<system_monitoring::SystemMonitor>`
//! ([`system_monitoring::SystemMonitor`]).
//!
mod api;
mod middleware;
mod process;
mod resource;
mod router;
mod server;
mod state;
mod system_monitoring;
mod web;
pub(super) use server::run;
use state::AppState;
#[cfg(test)]
mod process_api_tests;
#[cfg(test)]
mod system_api_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
