use super::{
    AppState, api,
    middleware::{guard_http, log_request},
    web,
};
use axum::{Router, middleware};
use std::{net::SocketAddr, sync::Arc};

pub(super) fn router(state: Arc<AppState>, address: SocketAddr) -> Router {
    web::router()
        .merge(api::router())
        .layer(middleware::from_fn(move |request, next| {
            guard_http(request, next, address)
        }))
        .layer(middleware::from_fn(log_request))
        .with_state(state)
}
