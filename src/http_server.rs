//! HTTP server wiring and shared API.
//!
//! # Interface
//!
//! | Definition | Visibility | Signature |
//! | --- | --- | --- |
//! | [`run`] | `pub(super)` | `async fn run(listen: `[`SocketAddr`](std::net::SocketAddr)`, interval: `[`Duration`](std::time::Duration)`) -> `[`anyhow::Result`]`<()>` |
//!
//! Shared state: [`state::AppState::system_monitor`] is
//! `pub(super) system_monitor: Arc<system_monitoring::SystemMonitor>`
//! ([`system_monitoring::SystemMonitor`]).
//!
mod api;
mod middleware;
mod process;
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
