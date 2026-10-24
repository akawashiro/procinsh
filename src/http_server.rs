//! HTTP application with cleared capabilities and helper supervision.
//!
//! # Interface
//! [`run`] (`pub(crate)`): `fn run(listen: &[std::net::SocketAddr]) -> anyhow::Result<()>`.
mod api;
mod client;
mod delivery;
mod launch;
mod middleware;
mod privileges;
mod router;
mod server;
mod state;
mod web;
pub(crate) use launch::run;
use state::AppState;
