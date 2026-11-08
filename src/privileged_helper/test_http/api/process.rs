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
use {
    super::super::AppState,
    crate::{ipc::schema::ProcessId, privileged_helper::process},
};
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

async fn run_blocking_request<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, ApiError> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
}

pub(super) async fn get_server_config(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(
        json!({"version":env!("CARGO_PKG_VERSION"),"interval_ms":s.interval.as_millis(),"history_seconds":60}),
    )
}

pub(super) async fn list_processes() -> ApiResult {
    run_blocking_request(move || Ok(Json(json!(process::collect_processes()?)))).await
}

pub(super) async fn get_process_observation(Query(id): Query<ProcessId>) -> ApiResult {
    run_blocking_request(move || Ok(Json(json!(process::collect_process_observation(id)?)))).await
}

pub(super) async fn get_process_threads(Query(id): Query<ProcessId>) -> ApiResult {
    run_blocking_request(move || Ok(Json(json!(process::collect_thread_observation(id)?)))).await
}

pub(super) async fn get_process_maps(Query(id): Query<ProcessId>) -> ApiResult {
    run_blocking_request(move || Ok(Json(json!(process::collect_memory_maps(id)?)))).await
}

pub(super) async fn get_process_environment(Query(id): Query<ProcessId>) -> ApiResult {
    run_blocking_request(move || {
        process::validate_process_identity(id)?;
        Ok(Json(json!(process::read_environment(id)?)))
    })
    .await
}

pub(super) async fn get_process_file_descriptors(Query(id): Query<ProcessId>) -> ApiResult {
    run_blocking_request(move || {
        process::validate_process_identity(id)?;
        Ok(Json(json!(process::collect_file_descriptors(id)?)))
    })
    .await
}

pub(super) async fn get_process_auxiliary_vector(Query(id): Query<ProcessId>) -> ApiResult {
    run_blocking_request(move || {
        process::validate_process_identity(id)?;
        Ok(Json(json!(process::read_auxiliary_vector(id)?)))
    })
    .await
}

pub(super) async fn stream_process_events(
    State(s): State<Arc<AppState>>,
    Query(id): Query<ProcessId>,
) -> Result<Response, ApiError> {
    use crate::privileged_helper::process::{
        observe_process_monitor, process_monitor_is_stopped, reserve_process_monitor,
    };

    let permit = reserve_process_monitor(&s.monitoring).map_err(|error| {
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
    let mut session =
        run_blocking_request(move || Ok(observe_process_monitor(&state, id, permit)?)).await?;
    let initial = session.receiver.borrow_and_update().clone();
    let stream = async_stream::stream! {
        log::debug!("SSE /api/processes/events event=observation pid={} start_time_ticks={}", id.pid, id.start_time_ticks);
        yield Ok::<_, Infallible>(Event::default().event("observation").json_data(initial).unwrap());
        loop {
            if process_monitor_is_stopped(&s.monitoring) { break; }
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
