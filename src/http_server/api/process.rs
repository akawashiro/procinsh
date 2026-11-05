use super::super::AppState;

use super::hosts::{HostQuery, client};
use crate::http_server::protocol::{MonitorRequest, MonitorResponse, ProcessId, ProcessResource};
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

pub(super) async fn config(State(s): State<Arc<AppState>>) -> Json<Value> {
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

async fn request(state: &AppState, host: Option<&str>, request: MonitorRequest) -> ApiResult {
    match client(state, host)?.request(request).await? {
        MonitorResponse::Data(value) => Ok(Json(value)),
        _ => Err(anyhow::anyhow!("unexpected monitor IPC response").into()),
    }
}

pub(super) async fn list_processes(
    State(s): State<Arc<AppState>>,
    Query(query): Query<HostQuery>,
) -> ApiResult {
    request(&s, query.host.as_deref(), MonitorRequest::Processes).await
}

pub(super) async fn stats(
    State(s): State<Arc<AppState>>,
    Query(query): Query<ProcessQuery>,
) -> ApiResult {
    resource(&s, query, ProcessResource::Observation).await
}
pub(super) async fn threads(
    State(s): State<Arc<AppState>>,
    Query(query): Query<ProcessQuery>,
) -> ApiResult {
    resource(&s, query, ProcessResource::Threads).await
}
pub(super) async fn maps(
    State(s): State<Arc<AppState>>,
    Query(query): Query<ProcessQuery>,
) -> ApiResult {
    resource(&s, query, ProcessResource::Maps).await
}
pub(super) async fn environment(
    State(s): State<Arc<AppState>>,
    Query(query): Query<ProcessQuery>,
) -> ApiResult {
    resource(&s, query, ProcessResource::Environment).await
}
pub(super) async fn fds(
    State(s): State<Arc<AppState>>,
    Query(query): Query<ProcessQuery>,
) -> ApiResult {
    resource(&s, query, ProcessResource::Fds).await
}
pub(super) async fn auxv(
    State(s): State<Arc<AppState>>,
    Query(query): Query<ProcessQuery>,
) -> ApiResult {
    resource(&s, query, ProcessResource::Auxv).await
}
async fn resource(state: &AppState, query: ProcessQuery, resource: ProcessResource) -> ApiResult {
    let id = query.id();
    request(
        state,
        query.host.as_deref(),
        MonitorRequest::Process { id, resource },
    )
    .await
}

pub(super) async fn events(
    State(s): State<Arc<AppState>>,
    Query(query): Query<ProcessQuery>,
) -> Result<Response, ApiError> {
    let session = client(&s, query.host.as_deref())?
        .observe(MonitorRequest::ObserveProcess { id: query.id() })
        .await?;
    Ok(super::stream::response(s, session))
}
