use std::{sync::Arc, time::Duration};
#[tokio::test]
async fn sse_connections_own_viewer_lifetimes() {
    use super::AppState;
    use axum::{
        body::{Body, HttpBody},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;
    let state = Arc::new(AppState::new(Duration::from_secs(1)));
    let app = super::router::router(state.clone(), "127.0.0.1:8080".parse().unwrap());
    let request = |path: &str| {
        Request::builder()
            .uri(path)
            .header("host", "127.0.0.1:8080")
            .body(Body::empty())
            .unwrap()
    };
    for path in ["/api/system/status", "/api/system/snapshot"] {
        assert_eq!(
            app.clone().oneshot(request(path)).await.unwrap().status(),
            StatusCode::NOT_FOUND
        );
        assert!(!state.system.active());
    }
    for method in ["POST", "DELETE"] {
        let mut req = request("/api/system/leases");
        *req.method_mut() = method.parse().unwrap();
        assert_eq!(
            app.clone().oneshot(req).await.unwrap().status(),
            StatusCode::NOT_FOUND
        );
    }
    let mut responses = Vec::new();
    for _ in 0..32 {
        let response = app
            .clone()
            .oneshot(request("/api/system/events"))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        responses.push(response);
    }
    assert!(state.system.active());
    assert_eq!(
        app.clone()
            .oneshot(request("/api/system/events"))
            .await
            .unwrap()
            .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    // Unpolled response bodies must also release their registration.
    drop(responses.pop());
    let response = app
        .clone()
        .oneshot(request("/api/system/events"))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body();
    let frame = std::future::poll_fn(|cx| std::pin::Pin::new(&mut body).poll_frame(cx))
        .await
        .unwrap()
        .unwrap()
        .into_data()
        .unwrap();
    assert!(
        std::str::from_utf8(&frame)
            .unwrap()
            .contains("event: snapshot")
    );
    let text = std::str::from_utf8(&frame).unwrap();
    let payload: serde_json::Value = serde_json::from_str(
        text.lines()
            .find_map(|line| line.strip_prefix("data: "))
            .unwrap(),
    )
    .unwrap();
    assert!(payload["processes"].is_array());
    assert!(payload["fd_relations"].is_array());
    assert!(payload.get("nodes").is_none());
    assert!(payload.get("edges").is_none());
    responses.clear();
    assert!(state.system.active());
    drop(body);
    assert!(!state.system.active());
    let response = app
        .clone()
        .oneshot(request("/api/system/events"))
        .await
        .unwrap();
    assert!(state.system.active());
    state.stop();
    assert_eq!(
        app.clone()
            .oneshot(request("/api/system/events"))
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    tokio::time::timeout(
        Duration::from_secs(3),
        axum::body::to_bytes(response.into_body(), usize::MAX),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!state.system.active());
    state.join_collectors().unwrap();
}
