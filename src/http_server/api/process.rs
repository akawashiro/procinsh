//! Serve process metadata and observation HTTP endpoints.
//!
//! # Interface
//! - [`ApiError`] (`pub(super)`): `struct ApiError`.
//! - [`ProcessQuery`] (`pub(super)`): `struct ProcessQuery`.
//! - [`get_server_config`] (`pub(super)`): `async fn get_server_config(State(s): State<Arc<AppState>>) -> Json<Value>`.
//! - [`list_processes`] (`pub(super)`): `async fn list_processes( State(s): State<Arc<AppState>>, Query(query): Query<HostQuery>, ) -> ApiResult`.
//! - [`get_process_observation`] (`pub(super)`): `async fn get_process_observation( State(s): State<Arc<AppState>>, Query(query): Query<ProcessQuery>, ) -> ApiResult`.
//! - [`get_process_threads`] (`pub(super)`): `async fn get_process_threads( State(s): State<Arc<AppState>>, Query(query): Query<ProcessQuery>, ) -> ApiResult`.
//! - [`get_process_maps`] (`pub(super)`): `async fn get_process_maps( State(s): State<Arc<AppState>>, Query(query): Query<ProcessQuery>, ) -> ApiResult`.
//! - [`get_process_environment`] (`pub(super)`): `async fn get_process_environment( State(s): State<Arc<AppState>>, Query(query): Query<ProcessQuery>, ) -> ApiResult`.
//! - [`get_process_file_descriptors`] (`pub(super)`): `async fn get_process_file_descriptors( State(s): State<Arc<AppState>>, Query(query): Query<ProcessQuery>, ) -> ApiResult`.
//! - [`get_process_auxiliary_vector`] (`pub(super)`): `async fn get_process_auxiliary_vector( State(s): State<Arc<AppState>>, Query(query): Query<ProcessQuery>, ) -> ApiResult`.
//! - [`stream_process_events`] (`pub(super)`): `async fn stream_process_events( State(s): State<Arc<AppState>>, Query(query): Query<ProcessQuery>, ) -> Result<Response, ApiError>`.
//!
//! Supporting types: [`ApiError`], [`ApiResult`], [`AppState`], [`Arc`], [`HostQuery`], [`Json`], [`ProcessQuery`], [`Query`], [`Response`], [`State`], [`Value`].

use super::super::AppState;
use super::hosts::{HostQuery, resolve_host_client};
use crate::{
    http_server::protocol::MonitorResponse,
    ipc::schema::{MonitorRequest, ProcessId, ProcessResource},
};
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;

pub(super) struct ApiError(StatusCode, String);
impl ApiError {
    pub(super) fn new(status: StatusCode, message: String) -> Self {
        Self(status, message)
    }
}
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        log::debug!("API error status={} detail={}", self.0, self.1);
        (self.0, Json(json!({"error":self.1}))).into_response()
    }
}
impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        let message = format!("{e:#}");
        let status = if message.contains("Server is stopping")
            || message.contains("monitor IPC")
            || message.contains("helper unavailable")
            || message.contains("agent link")
        {
            StatusCode::SERVICE_UNAVAILABLE
        } else if message.contains("Too many viewers") {
            StatusCode::TOO_MANY_REQUESTS
        } else if message.contains("Process exited") {
            StatusCode::GONE
        } else {
            StatusCode::UNPROCESSABLE_ENTITY
        };
        Self(status, message)
    }
}
type ApiResult = Result<Json<Value>, ApiError>;

pub(super) async fn get_server_config(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(
        json!({"version":env!("CARGO_PKG_VERSION"),"interval_ms":s.interval.as_millis(),"history_seconds":60}),
    )
}

/// A process on `host`, which defaults to this machine.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProcessQuery {
    host: Option<String>,
    pid: i32,
    start_time_ticks: u64,
}
impl ProcessQuery {
    fn id(&self) -> ProcessId {
        ProcessId {
            pid: self.pid,
            start_time_ticks: self.start_time_ticks,
        }
    }
}

async fn request_process_data(
    state: &AppState,
    host: Option<&str>,
    request: MonitorRequest,
) -> ApiResult {
    match resolve_host_client(state, host)?.request(request).await? {
        MonitorResponse::Data(value) => Ok(Json(value)),
        _ => Err(anyhow::anyhow!("unexpected monitor IPC response").into()),
    }
}

pub(super) async fn list_processes(
    State(s): State<Arc<AppState>>,
    Query(query): Query<HostQuery>,
) -> ApiResult {
    request_process_data(&s, query.host.as_deref(), MonitorRequest::Processes).await
}

pub(super) async fn get_process_observation(
    State(s): State<Arc<AppState>>,
    Query(query): Query<ProcessQuery>,
) -> ApiResult {
    request_process_resource(&s, query, ProcessResource::Observation).await
}

pub(super) async fn get_process_threads(
    State(s): State<Arc<AppState>>,
    Query(query): Query<ProcessQuery>,
) -> ApiResult {
    request_process_resource(&s, query, ProcessResource::Threads).await
}

pub(super) async fn get_process_maps(
    State(s): State<Arc<AppState>>,
    Query(query): Query<ProcessQuery>,
) -> ApiResult {
    request_process_resource(&s, query, ProcessResource::Maps).await
}

pub(super) async fn get_process_environment(
    State(s): State<Arc<AppState>>,
    Query(query): Query<ProcessQuery>,
) -> ApiResult {
    request_process_resource(&s, query, ProcessResource::Environment).await
}

pub(super) async fn get_process_file_descriptors(
    State(s): State<Arc<AppState>>,
    Query(query): Query<ProcessQuery>,
) -> ApiResult {
    request_process_resource(&s, query, ProcessResource::Fds).await
}

pub(super) async fn get_process_auxiliary_vector(
    State(s): State<Arc<AppState>>,
    Query(query): Query<ProcessQuery>,
) -> ApiResult {
    request_process_resource(&s, query, ProcessResource::Auxv).await
}

async fn request_process_resource(
    state: &AppState,
    query: ProcessQuery,
    resource: ProcessResource,
) -> ApiResult {
    let id = query.id();
    request_process_data(
        state,
        query.host.as_deref(),
        MonitorRequest::Process { id, resource },
    )
    .await
}

pub(super) async fn stream_process_events(
    State(s): State<Arc<AppState>>,
    Query(query): Query<ProcessQuery>,
) -> Result<Response, ApiError> {
    let session = resolve_host_client(&s, query.host.as_deref())?
        .observe(MonitorRequest::ObserveProcess { id: query.id() })
        .await?;
    Ok(super::stream::build_sse_response(s, session))
}
