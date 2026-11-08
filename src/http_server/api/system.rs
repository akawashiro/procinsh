//! Serve local, remote and merged system observation streams.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`stream_system_events`] | `pub(super)` | `async fn stream_system_events( `[`State`]`(s): `[`State`]`<`[`Arc`]`<`[`AppState`]`>>, `[`Query`]`(query): `[`Query`]`<`[`HostQuery`]`>, ) -> `[`Result`](anyhow::Result)`<`[`Response`]`, `[`ApiError`]`>` |

use super::super::{AppState, hosts::ALL_HOSTS};
use super::hosts::{HostQuery, resolve_host_client};
use super::process::ApiError;
use crate::ipc::schema::MonitorRequest;
use axum::{
    extract::{Query, State},
    response::Response,
};
use std::sync::Arc;

pub(super) async fn stream_system_events(
    State(s): State<Arc<AppState>>,
    Query(query): Query<HostQuery>,
) -> Result<Response, ApiError> {
    use crate::http_server::client::observe_monitor_client;

    if query.host.as_deref() == Some(ALL_HOSTS) {
        return super::merged::build_merged_event_response(s).await;
    }
    let session = observe_monitor_client(
        &(resolve_host_client(&s, query.host.as_deref())?),
        MonitorRequest::ObserveSystem,
    )
    .await?;
    Ok(super::stream::build_sse_response(s, session))
}
