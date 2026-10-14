use super::super::{AppState, privsep::MonitorRequest};
use super::process::ApiError;
use axum::{extract::State, response::Response};
use std::sync::Arc;

pub(super) async fn events(State(s): State<Arc<AppState>>) -> Result<Response, ApiError> {
    let session = s.client.observe(MonitorRequest::ObserveSystem).await?;
    Ok(super::stream::response(s, session))
}
