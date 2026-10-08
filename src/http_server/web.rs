//! Embedded list, process details, and SPACE pages and their static assets.
//!
//! # Interface
//! - [`router`]: `pub(super) fn router() -> Router<Arc<AppState>>`; supplies the
//!   page and asset routes for [`super::AppState`].

use super::AppState;
use axum::{Router, http::header, response::Html, routing::get};
use std::sync::Arc;

fn versioned_html(template: &str) -> Html<String> {
    Html(template.replace("{{PROCINSH_VERSION}}", env!("CARGO_PKG_VERSION")))
}

pub(super) fn router() -> Router<Arc<AppState>> {
    let mut router = Router::new()
        .route(
            "/",
            get(|| async { versioned_html(include_str!("../web/list/index.html")) }),
        )
        .route(
            "/list",
            get(|| async { versioned_html(include_str!("../web/list/index.html")) }),
        )
        .route(
            "/process/{pid}",
            get(|| async { versioned_html(include_str!("../web/process/index.html")) }),
        )
        .route(
            "/space",
            get(|| async { versioned_html(include_str!("../web/space/index.html")) }),
        );
    for (path, source) in [
        ("/list/app.js", include_str!("../../dist/web/list/app.js")),
        (
            "/process/app.js",
            include_str!("../../dist/web/process/app.js"),
        ),
        ("/list/data.js", include_str!("../../dist/web/list/data.js")),
        (
            "/list/renderer.js",
            include_str!("../../dist/web/list/renderer.js"),
        ),
        (
            "/list/search.js",
            include_str!("../../dist/web/list/search.js"),
        ),
        (
            "/list/dom-types.js",
            include_str!("../../dist/web/list/dom-types.js"),
        ),
        (
            "/process/data.js",
            include_str!("../../dist/web/process/data.js"),
        ),
        (
            "/process/renderer.js",
            include_str!("../../dist/web/process/renderer.js"),
        ),
        (
            "/process/selection.js",
            include_str!("../../dist/web/process/selection.js"),
        ),
        (
            "/process/samples.js",
            include_str!("../../dist/web/process/samples.js"),
        ),
        (
            "/process/history.js",
            include_str!("../../dist/web/process/history.js"),
        ),
        (
            "/process/details.js",
            include_str!("../../dist/web/process/details.js"),
        ),
        (
            "/process/search.js",
            include_str!("../../dist/web/process/search.js"),
        ),
        (
            "/process/dom-types.js",
            include_str!("../../dist/web/process/dom-types.js"),
        ),
        ("/space/app.js", include_str!("../../dist/web/space/app.js")),
        (
            "/space/data.js",
            include_str!("../../dist/web/space/data.js"),
        ),
        (
            "/space/scene.js",
            include_str!("../../dist/web/space/scene.js"),
        ),
        (
            "/space/renderer.js",
            include_str!("../../dist/web/space/renderer.js"),
        ),
        (
            "/space/search.js",
            include_str!("../../dist/web/space/search.js"),
        ),
        (
            "/space/selection.js",
            include_str!("../../dist/web/space/selection.js"),
        ),
        (
            "/space/camera.js",
            include_str!("../../dist/web/space/camera.js"),
        ),
        (
            "/space/details.js",
            include_str!("../../dist/web/space/details.js"),
        ),
        (
            "/space/dom-types.js",
            include_str!("../../dist/web/space/dom-types.js"),
        ),
        (
            "/shared/api.js",
            include_str!("../../dist/web/shared/api.js"),
        ),
        (
            "/shared/display.js",
            include_str!("../../dist/web/shared/display.js"),
        ),
        (
            "/shared/dom.js",
            include_str!("../../dist/web/shared/dom.js"),
        ),
        (
            "/shared/navigation.js",
            include_str!("../../dist/web/shared/navigation.js"),
        ),
        (
            "/vendor/three.module.js",
            include_str!("../web/vendor/three.module.js"),
        ),
        (
            "/vendor/three.core.js",
            include_str!("../web/vendor/three.core.js"),
        ),
        (
            "/vendor/OrbitControls.js",
            include_str!("../web/vendor/OrbitControls.js"),
        ),
    ] {
        router = router.route(
            path,
            get(move || async move {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    source,
                )
            }),
        );
    }
    for (path, source) in [
        ("/list/style.css", include_str!("../web/list/style.css")),
        (
            "/process/style.css",
            include_str!("../web/process/style.css"),
        ),
        ("/space/style.css", include_str!("../web/space/style.css")),
        ("/shared/style.css", include_str!("../web/shared/style.css")),
    ] {
        router =
            router.route(
                path,
                get(move || async move {
                    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], source)
                }),
            );
    }
    router
}
