//! Serve local, remote and merged system observation streams.
//!
//! # Interface
//! - [`stream_system_events`] (`pub(super)`): `async fn stream_system_events( State(s): State<Arc<AppState>>, Query(query): Query<HostQuery>, ) -> Result<Response, ApiError>`.
//!
//! Supporting types: [`ApiError`], [`AppState`], [`Arc`], [`HostQuery`], [`Query`], [`Response`], [`State`].

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
    if query.host.as_deref() == Some(ALL_HOSTS) {
        return super::merged::build_merged_event_response(s).await;
    }
    let session = resolve_host_client(&s, query.host.as_deref())?
        .observe(MonitorRequest::ObserveSystem)
        .await?;
    Ok(super::stream::build_sse_response(s, session))
}
