use super::super::{
    AppState,
    privsep::{MonitorRequest, MonitorResponse, ProcessResource},
    process::ProcessId,
};
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use std::sync::Arc;

pub(super) struct ApiError(StatusCode, String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        log::debug!("API error status={} detail={}", self.0, self.1);
        (self.0, Json(json!({"error":self.1}))).into_response()
    }
}
impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        let message = format!("{e:#}");
        let status = if message.contains("Server is stopping") || message.contains("monitor IPC") {
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

async fn request(state: &AppState, request: MonitorRequest) -> ApiResult {
    match state.client.request(request).await? {
        MonitorResponse::Data(value) => Ok(Json(value)),
        _ => Err(anyhow::anyhow!("unexpected monitor IPC response").into()),
    }
}

pub(super) async fn processes(State(s): State<Arc<AppState>>) -> ApiResult {
    request(&s, MonitorRequest::Processes).await
}

pub(super) async fn stats(
    State(s): State<Arc<AppState>>,
    Query(id): Query<ProcessId>,
) -> ApiResult {
    request(
        &s,
        MonitorRequest::Process {
            id,
            resource: ProcessResource::Observation,
        },
    )
    .await
}
pub(super) async fn threads(
    State(s): State<Arc<AppState>>,
    Query(id): Query<ProcessId>,
) -> ApiResult {
    request(
        &s,
        MonitorRequest::Process {
            id,
            resource: ProcessResource::Threads,
        },
    )
    .await
}
pub(super) async fn maps(State(s): State<Arc<AppState>>, Query(id): Query<ProcessId>) -> ApiResult {
    request(
        &s,
        MonitorRequest::Process {
            id,
            resource: ProcessResource::Maps,
        },
    )
    .await
}
pub(super) async fn environment(
    State(s): State<Arc<AppState>>,
    Query(id): Query<ProcessId>,
) -> ApiResult {
    request(
        &s,
        MonitorRequest::Process {
            id,
            resource: ProcessResource::Environment,
        },
    )
    .await
}
pub(super) async fn fds(State(s): State<Arc<AppState>>, Query(id): Query<ProcessId>) -> ApiResult {
    request(
        &s,
        MonitorRequest::Process {
            id,
            resource: ProcessResource::Fds,
        },
    )
    .await
}
pub(super) async fn auxv(State(s): State<Arc<AppState>>, Query(id): Query<ProcessId>) -> ApiResult {
    request(
        &s,
        MonitorRequest::Process {
            id,
            resource: ProcessResource::Auxv,
        },
    )
    .await
}
pub(super) async fn events(
    State(s): State<Arc<AppState>>,
    Query(id): Query<ProcessId>,
) -> Result<Response, ApiError> {
    let session = s
        .client
        .observe(MonitorRequest::ObserveProcess { id })
        .await?;
    Ok(super::stream::response(s, session))
}
