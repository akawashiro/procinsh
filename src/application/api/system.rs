use super::super::AppState;
use super::process::ApiError;
use axum::{extract::State, response::Response};
use procinsh_protocol::protocol::MonitorRequest;
use std::sync::Arc;

pub(super) async fn events(State(s): State<Arc<AppState>>) -> Result<Response, ApiError> {
    let session = s.client.observe(MonitorRequest::ObserveSystem).await?;
    Ok(super::stream::response(s, session))
}
