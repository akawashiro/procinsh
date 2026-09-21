use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use procinsh::state::AppState;
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;

#[tokio::test]
async fn generated_javascript_is_embedded_at_existing_urls() {
    let state = Arc::new(AppState::new(Duration::from_secs(1)));
    let app = procinsh::server::router(state, "127.0.0.1:8080".parse().unwrap());
    for (path, expected) in [
        ("/app.js", include_str!("../dist/web/app.js")),
        ("/space.js", include_str!("../dist/web/space.js")),
        (
            "/space-model.js",
            include_str!("../dist/web/space-model.js"),
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("host", "127.0.0.1:8080")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200, "{path}");
        assert_eq!(
            response.headers()["content-type"],
            "text/javascript; charset=utf-8"
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(body.as_ref(), expected.as_bytes(), "{path}");
    }
}
