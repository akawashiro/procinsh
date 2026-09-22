use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use procinsh::state::AppState;
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;

#[tokio::test]
async fn target_state_is_delivered_by_sse() {
    use axum::body::HttpBody;
    let state = Arc::new(AppState::new(Duration::from_secs(1)));
    let app = procinsh::server::router(state.clone(), "127.0.0.1:8080".parse().unwrap());
    let request = |path: &str| {
        Request::builder()
            .uri(path)
            .header("host", "127.0.0.1:8080")
            .body(Body::empty())
            .unwrap()
    };
    assert_eq!(
        app.clone()
            .oneshot(request("/api/target"))
            .await
            .unwrap()
            .status(),
        405
    );
    let response = app
        .clone()
        .oneshot(request("/api/target/events"))
        .await
        .unwrap();
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    let mut body = response.into_body();
    async fn next(body: &mut Body) -> serde_json::Value {
        let frame = tokio::time::timeout(
            Duration::from_secs(2),
            std::future::poll_fn(|cx| std::pin::Pin::new(&mut *body).poll_frame(cx)),
        )
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .into_data()
        .unwrap();
        let text = std::str::from_utf8(&frame).unwrap();
        assert!(text.contains("event: observation"));
        serde_json::from_str(
            text.lines()
                .find_map(|line| line.strip_prefix("data: "))
                .unwrap(),
        )
        .unwrap()
    }
    assert!(next(&mut body).await.is_null());
    let id = procinsh::process::identity(std::process::id() as i32).unwrap();
    for method in ["POST", "DELETE"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri("/api/target")
                    .header("host", "127.0.0.1:8080")
                    .header("content-type", "application/json")
                    .body(Body::from(serde_json::to_vec(&id).unwrap()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let expected: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        assert_eq!(next(&mut body).await, expected);
        let mut fresh = app
            .clone()
            .oneshot(request("/api/target/events"))
            .await
            .unwrap()
            .into_body();
        assert_eq!(next(&mut fresh).await, expected);
    }
    state.stop();
}

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
