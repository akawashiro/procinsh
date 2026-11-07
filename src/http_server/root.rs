//! HTTP application with cleared capabilities, observation analysis and helper supervision.
//!
//! # Interface
//! - [`launch_http_server`] (`pub(crate)`): `fn launch_http_server(config: ServerConfig) -> anyhow::Result<()>`.
//! - [`ServerConfig`] (`pub(crate)`): listeners, agent links, the host name and privacy mode.
//! - [`valid_host_name`] (`pub(crate)`): `fn valid_host_name(name: &str) -> bool`.
#[path = "analysis/root.rs"]
mod analysis;
mod api;
mod client;
mod delivery;
mod disasm;
mod hosts;
#[cfg(test)]
mod hosts_link_tests;
mod json;
mod launch;
mod middleware;
#[path = "model/root.rs"]
mod model;
mod privacy;
mod privileges;
mod processing;
mod router;
mod server;
mod state;
mod web;
pub(crate) use hosts::valid_host_name;
pub(crate) use launch::launch_http_server;
pub(crate) use server::ServerConfig;
use state::AppState;

mod protocol;

mod elf_reader;
