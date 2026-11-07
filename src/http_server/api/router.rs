use super::{hosts, process, system};
use crate::http_server::AppState;
use axum::{Router, routing::get};
use std::sync::Arc;

pub(in crate::http_server) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/api/system/events", get(system::stream_system_events))
        .route("/api/config", get(process::get_server_config))
        .route("/api/hosts", get(hosts::list_hosts))
        .route("/api/processes", get(process::list_processes))
        .route(
            "/api/processes/observation",
            get(process::get_process_observation),
        )
        .route("/api/processes/threads", get(process::get_process_threads))
        .route("/api/processes/maps", get(process::get_process_maps))
        .route(
            "/api/processes/environment",
            get(process::get_process_environment),
        )
        .route(
            "/api/processes/auxv",
            get(process::get_process_auxiliary_vector),
        )
        .route(
            "/api/processes/fds",
            get(process::get_process_file_descriptors),
        )
        .route("/api/processes/events", get(process::stream_process_events))
}
