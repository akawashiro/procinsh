//! HTTP server wiring and shared API.
//!
//! # Interface
//!
//! Names link to the defining item's documentation; its **Source** link opens the implementation.
//! Re-export visibility: `pub(super)`.
//!
//! | Definition | Signature |
//! | --- | --- |
//! | [`run`] | `async fn run(listen: SocketAddr, interval: Duration) -> anyhow::Result<()>` |
//!
//! Types: [`SocketAddr`](std::net::SocketAddr), [`Duration`](std::time::Duration).
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
