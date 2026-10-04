use super::AppState;
use axum::{Router, http::header, response::Html, routing::get};
use std::sync::Arc;

fn versioned_html(template: &str) -> Html<String> {
    Html(template.replace("{{PROCINSH_VERSION}}", env!("CARGO_PKG_VERSION")))
}

pub(super) fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route(
            "/",
            get(|| async { versioned_html(include_str!("../web/index.html")) }),
        )
        .route(
            "/list",
            get(|| async { versioned_html(include_str!("../web/index.html")) }),
        )
        .route(
            "/process/{pid}",
            get(|| async { versioned_html(include_str!("../web/index.html")) }),
        )
        .route(
            "/display.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../../dist/web/display.js"),
                )
            }),
        )
        .route(
            "/app.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../../dist/web/app.js"),
                )
            }),
        )
        .route(
            "/style.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    include_str!("../web/style.css"),
                )
            }),
        )
        .route(
            "/space",
            get(|| async { versioned_html(include_str!("../web/space.html")) }),
        )
        .route(
            "/space.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../../dist/web/space.js"),
                )
            }),
        )
        .route(
            "/space-model.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../../dist/web/space-model.js"),
                )
            }),
        )
        .route(
            "/space.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    include_str!("../web/space.css"),
                )
            }),
        )
        .route(
            "/vendor/three.module.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../web/vendor/three.module.js"),
                )
            }),
        )
        .route(
            "/vendor/three.core.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../web/vendor/three.core.js"),
                )
            }),
        )
        .route(
            "/vendor/OrbitControls.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../web/vendor/OrbitControls.js"),
                )
            }),
        )
}
