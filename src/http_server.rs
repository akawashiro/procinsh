//! HTTP server wiring and shared API.
//!
//! # Interface
//!
//! | Definition | Visibility | Signature |
//! | --- | --- | --- |
//! | [`run`] | `pub(super)` | `async fn run(listen: &[`[`SocketAddr`](std::net::SocketAddr)`]) -> `[`anyhow::Result`]`<()>` |
//!
//! Shared resource identities live in [`resource`]; socket metadata and descriptor
//! capabilities live in [`socket_types`].
//!
mod api;
mod middleware;
mod process;
mod resource;
mod router;
mod server;
mod socket_types;
mod state;
mod system;
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
