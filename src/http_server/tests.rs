use super::{AppState, router::router};
use axum::{Router, extract::Request};
use std::{sync::Arc, time::Duration};

use axum::body::Body;
use tower::ServiceExt;

fn app() -> Router {
    router(
        Arc::new(AppState::new(Duration::from_secs(1))),
        "127.0.0.1:8080".parse().unwrap(),
    )
}

#[tokio::test]
async fn security_and_embedded_resources() {
    for (host, origin, path, status) in [
        ("evil.test:8080", None, "/api/processes", 403),
        (
            "127.0.0.1:8080",
            Some("https://evil.test"),
            "/api/processes",
            403,
        ),
        ("127.0.0.1:8080", None, "/", 200),
        ("127.0.0.1:8080", None, "/list", 200),
        ("localhost:8080", None, "/app.js", 200),
    ] {
        let mut req = Request::builder().uri(path).header("host", host);
        if let Some(origin) = origin {
            req = req.header("origin", origin);
        }
        let response = app()
            .oneshot(req.body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status);
    }
}
