use super::super::{AppState, hosts::ALL_HOSTS};

use super::hosts::{HostQuery, client};
use super::process::ApiError;
use crate::http_server::protocol::MonitorRequest;
use axum::{
    extract::{Query, State},
    response::Response,
};
use std::sync::Arc;

pub(super) async fn events(
    State(s): State<Arc<AppState>>,
    Query(query): Query<HostQuery>,
) -> Result<Response, ApiError> {
    if query.host.as_deref() == Some(ALL_HOSTS) {
        return super::merged::response(s).await;
    }
    let session = client(&s, query.host.as_deref())?
        .observe(MonitorRequest::ObserveSystem)
        .await?;
    Ok(super::stream::response(s, session))
}
