//! Unprivileged HTTP application and helper supervision.
//!
//! # Interface
//! [`run`] (`pub(crate)`): `fn run(listen: &[std::net::SocketAddr]) -> anyhow::Result<()>`.
mod api;
mod client;
mod delivery;
mod launch;
#[path = "http_server/middleware.rs"]
mod middleware;
mod privileges;
#[path = "http_server/router.rs"]
mod router;
mod server;
mod state;
#[path = "http_server/web.rs"]
mod web;
pub(crate) use launch::run;
use state::AppState;
