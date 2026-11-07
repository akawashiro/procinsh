//! List known hosts and resolve the helper client serving a requested host.
//!
//! # Interface
//! - [`HostQuery`] (`pub(super)`): `struct HostQuery`.
//! - [`list_hosts`] (`pub(super)`): `async fn list_hosts(`[`State`]`(s): `[`State`]`<`[`Arc`]`<`[`AppState`]`>>) -> `[`Json`]`<`[`Value`]`>`.
//! - [`resolve_host_client`] (`pub(super)`): `fn resolve_host_client( s: &`[`AppState`]`, host: `[`Option`]`<&str>, ) -> `[`Result`]`<`[`MonitorClient`]`, `[`ApiError`]`>`.

use super::super::{
    AppState,
    client::MonitorClient,
    hosts::{ALL_HOSTS, RemoteHost},
};
use super::process::ApiError;
use axum::{Json, extract::State, http::StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

/// `?host=`: a host name; absent or the local name selects this machine.
#[derive(Deserialize)]
pub(super) struct HostQuery {
    pub(super) host: Option<String>,
}

/// `{"local": name, "hosts": [summary...]}` with the local host first.
pub(super) async fn list_hosts(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(json!({
        "local": s.hosts.local().name,
        "hosts": s.hosts.summaries(),
    }))
}

/// The helper of `host`: this machine's own, or a remote host's through its agent link.
pub(super) fn resolve_host_client(
    s: &AppState,
    host: Option<&str>,
) -> Result<MonitorClient, ApiError> {
    let Some(host) = host.filter(|host| *host != s.hosts.local().name) else {
        return Ok(s.client.clone());
    };
    match s.hosts.remote(host) {
        RemoteHost::Online(link) => Ok(MonitorClient::agent(link)),
        RemoteHost::Offline => Err(ApiError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            format!("Host {host} is offline"),
        )),
        RemoteHost::Unknown if host == ALL_HOSTS => Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "Only SPACE events support all hosts".into(),
        )),
        RemoteHost::Unknown => Err(ApiError::new(
            StatusCode::NOT_FOUND,
            format!("Unknown host {host}"),
        )),
    }
}
