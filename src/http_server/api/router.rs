use super::{process, system};
use crate::http_server::AppState;
use axum::{Router, routing::get};
use std::sync::Arc;

pub(in crate::http_server) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/system/events", get(system::events))
        .route("/api/config", get(process::config))
        .route("/api/processes", get(process::processes))
        .route("/api/processes/observation", get(process::stats))
        .route("/api/processes/threads", get(process::threads))
        .route("/api/processes/maps", get(process::maps))
        .route("/api/processes/environment", get(process::environment))
        .route("/api/processes/auxv", get(process::auxv))
        .route("/api/processes/fds", get(process::fds))
        .route("/api/processes/events", get(process::events))
}
