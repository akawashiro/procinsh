use super::{process, system};
use crate::privileged_helper::AppState;
use axum::{Router, routing::get};
use std::sync::Arc;

pub(in crate::privileged_helper) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/system/events", get(system::events))
        .route("/api/config", get(process::config))
        .route("/api/processes", get(process::list_processes))
        .route("/api/processes/observation", get(process::stats))
        .route("/api/processes/threads", get(process::threads))
        .route("/api/processes/maps", get(process::maps))
        .route("/api/processes/environment", get(process::environment))
        .route("/api/processes/auxv", get(process::auxv))
        .route("/api/processes/fds", get(process::fds))
        .route("/api/processes/events", get(process::events))
}
