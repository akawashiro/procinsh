//! List known hosts and resolve the helper client serving a requested host.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`HostQuery`] | `pub(super)` | `struct HostQuery` |
//! | [`list_hosts`] | `pub(super)` | `async fn list_hosts(`[`State`]`(s): `[`State`]`<`[`Arc`]`<`[`AppState`]`>>) -> `[`Json`]`<`[`Value`]`>` |
//! | [`resolve_host_client`] | `pub(super)` | `fn resolve_host_client( s: &`[`AppState`]`, host: `[`Option`]`<&str>, ) -> `[`Result`](anyhow::Result)`<`[`MonitorClient`]`, `[`ApiError`]`>` |

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
    use crate::http_server::hosts::{local_host_registry, summaries_host_registry};

    Json(json!({
        "local": local_host_registry(&s.hosts).name,
        "hosts": summaries_host_registry(&s.hosts),
    }))
}

/// The helper of `host`: this machine's own, or a remote host's through its agent link.
pub(super) fn resolve_host_client(
    s: &AppState,
    host: Option<&str>,
) -> Result<MonitorClient, ApiError> {
    use crate::http_server::{
        api::process::create_api_error,
        client::create_agent_monitor_client,
        hosts::{local_host_registry, remote_host_registry},
    };

    let Some(host) = host.filter(|host| *host != local_host_registry(&s.hosts).name) else {
        return Ok(s.client.clone());
    };
    match remote_host_registry(&s.hosts, host) {
        RemoteHost::Online(link) => Ok(create_agent_monitor_client(link)),
        RemoteHost::Offline => Err(create_api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            format!("Host {host} is offline"),
        )),
        RemoteHost::Unknown if host == ALL_HOSTS => Err(create_api_error(
            StatusCode::BAD_REQUEST,
            "Only SPACE events support all hosts".into(),
        )),
        RemoteHost::Unknown => Err(create_api_error(
            StatusCode::NOT_FOUND,
            format!("Unknown host {host}"),
        )),
    }
}
