use super::super::{
    AppState,
    process::{self, ProcessId},
};
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
};
use serde_json::{Value, json};
use std::{convert::Infallible, sync::Arc, time::Duration};
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
        let status = if message.contains("Server is stopping") {
            StatusCode::SERVICE_UNAVAILABLE
        } else if message.contains("Process exited") {
            StatusCode::GONE
        } else {
            StatusCode::UNPROCESSABLE_ENTITY
        };
        Self(status, message)
    }
}
type ApiResult = Result<Json<Value>, ApiError>;

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, ApiError> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
}

pub(super) async fn config(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(
        json!({"version":env!("CARGO_PKG_VERSION"),"interval_ms":s.interval.as_millis(),"history_seconds":60}),
    )
}

pub(super) async fn processes(State(s): State<Arc<AppState>>) -> ApiResult {
    blocking(move || Ok(Json(json!(s.discovery.lock().unwrap().collect()?)))).await
}

pub(super) async fn stats(Query(id): Query<ProcessId>) -> ApiResult {
    blocking(move || Ok(Json(json!(process::observation(id)?)))).await
}

pub(super) async fn threads(Query(id): Query<ProcessId>) -> ApiResult {
    blocking(move || Ok(Json(json!(process::threads(id)?)))).await
}

pub(super) async fn maps(Query(id): Query<ProcessId>) -> ApiResult {
    blocking(move || Ok(Json(json!(process::maps(id)?)))).await
}

pub(super) async fn environment(Query(id): Query<ProcessId>) -> ApiResult {
    blocking(move || {
        process::check_identity(id)?;
        Ok(Json(json!(process::environment(id)?)))
    })
    .await
}

pub(super) async fn fds(Query(id): Query<ProcessId>) -> ApiResult {
    blocking(move || {
        process::check_identity(id)?;
        Ok(Json(json!(process::fds(id)?)))
    })
    .await
}

pub(super) async fn signals(Query(id): Query<ProcessId>) -> ApiResult {
    blocking(move || {
        process::check_identity(id)?;
        Ok(Json(json!(process::signals(id)?)))
    })
    .await
}

pub(super) async fn auxv(Query(id): Query<ProcessId>) -> ApiResult {
    blocking(move || {
        process::check_identity(id)?;
        Ok(Json(json!(process::auxv(id)?)))
    })
    .await
}

pub(super) async fn snapshot(
    State(s): State<Arc<AppState>>,
    Json(id): Json<ProcessId>,
) -> ApiResult {
    blocking(move || Ok(Json(json!(s.snapshotter.capture(id)?)))).await
}

pub(super) async fn events(
    State(s): State<Arc<AppState>>,
    Query(id): Query<ProcessId>,
) -> Result<Response, ApiError> {
    let permit = s.monitoring.reserve().map_err(|error| {
        ApiError(
            match error {
                process::SubscribeError::Stopped => StatusCode::SERVICE_UNAVAILABLE,
                process::SubscribeError::TooManySubscribers => StatusCode::TOO_MANY_REQUESTS,
            },
            if error == process::SubscribeError::TooManySubscribers {
                "Too many viewers"
            } else {
                "Server is stopping"
            }
            .into(),
        )
    })?;
    let state = s.monitoring.clone();
    let mut session = blocking(move || Ok(state.observe(id, permit)?)).await?;
    let initial = session.receiver.borrow_and_update().clone();
    let stream = async_stream::stream! {
        log::debug!("SSE /api/processes/events event=observation pid={} start_time_ticks={}", id.pid, id.start_time_ticks);
        yield Ok::<_, Infallible>(Event::default().event("observation").json_data(initial).unwrap());
        loop {
            if s.monitoring.is_stopped() { break; }
            let received = tokio::select! {
                received = session.receiver.changed() => received,
                _ = tokio::time::sleep(Duration::from_millis(100)) => { continue; }
            };
            if received.is_err() { break; }
            let data = session.receiver.borrow_and_update().clone();
            let exited = data.exited;
            log::debug!("SSE /api/processes/events event=observation pid={} start_time_ticks={} exited={exited}", id.pid, id.start_time_ticks);
            yield Ok(Event::default().event("observation").json_data(data).unwrap());
            if exited { break; }
        }
        // Retain the permit for the complete stream lifetime, not just initialization.
        drop(session);
    };
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
        .into_response())
}
