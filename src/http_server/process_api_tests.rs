use super::AppState;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;

fn request(path: &str) -> Request<Body> {
    Request::builder()
        .uri(path)
        .header("host", "127.0.0.1:8080")
        .body(Body::empty())
        .unwrap()
}

fn query(id: super::process::ProcessId) -> String {
    format!("pid={}&start_time_ticks={}", id.pid, id.start_time_ticks)
}

async fn next(body: &mut Body) -> serde_json::Value {
    use axum::body::HttpBody;
    let frame = tokio::time::timeout(
        Duration::from_secs(3),
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

#[tokio::test]
async fn explicit_identity_is_required_without_selection() {
    let state = Arc::new(AppState::new(Duration::from_millis(100)));
    let app = super::router::router(state.clone(), "127.0.0.1:8080".parse().unwrap());
    let id = super::process::identity(std::process::id() as i32).unwrap();
    for method in ["GET", "POST", "DELETE"] {
        let mut req = request("/api/target");
        *req.method_mut() = method.parse().unwrap();
        assert_eq!(app.clone().oneshot(req).await.unwrap().status(), 404);
    }
    for path in [
        "/api/target/process",
        "/api/target/threads",
        "/api/target/maps",
        "/api/target/memory",
        "/api/target/fds",
        "/api/target/environment",
        "/api/target/auxv",
        "/api/target/signals",
        "/api/target/snapshot",
        "/api/target/events",
        "/api/space/status",
        "/api/space/snapshot",
        "/api/space/events",
        "/api/processes/process",
        "/api/system/snapshot",
    ] {
        let mut req = request(path);
        if path == "/api/target/snapshot" {
            *req.method_mut() = "POST".parse().unwrap();
        }
        assert_eq!(
            app.clone().oneshot(req).await.unwrap().status(),
            404,
            "{path}"
        );
    }
    assert_eq!(
        app.clone()
            .oneshot(request("/api/processes"))
            .await
            .unwrap()
            .status(),
        200
    );
    for path in [
        "observation",
        "threads",
        "maps",
        "fds",
        "environment",
        "auxv",
        "signals",
        "events",
    ] {
        for args in [
            "",
            "?pid=1",
            "?start_time_ticks=1",
            "?pid=abc&start_time_ticks=1",
        ] {
            let uri = format!("/api/processes/{path}{args}");
            assert_eq!(
                app.clone().oneshot(request(&uri)).await.unwrap().status(),
                400,
                "{uri}"
            );
        }
        let stale = super::process::ProcessId {
            start_time_ticks: id.start_time_ticks + 1,
            ..id
        };
        let uri = format!("/api/processes/{path}?{}", query(stale));
        assert_eq!(
            app.clone().oneshot(request(&uri)).await.unwrap().status(),
            410,
            "{uri}"
        );
    }
    for body in ["{}", "{\"pid\":1}", "{\"start_time_ticks\":1}"] {
        let mut req = request("/api/processes/snapshot");
        *req.method_mut() = "POST".parse().unwrap();
        req.headers_mut()
            .insert("content-type", "application/json".parse().unwrap());
        *req.body_mut() = Body::from(body);
        assert_eq!(app.clone().oneshot(req).await.unwrap().status(), 422);
    }
    for path in [
        "observation",
        "threads",
        "maps",
        "fds",
        "environment",
        "auxv",
        "signals",
    ] {
        let response = app
            .clone()
            .oneshot(request(&format!("/api/processes/{path}?{}", query(id))))
            .await
            .unwrap();
        assert_eq!(response.status(), 200, "{path}");
        let json: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap())
                .unwrap();
        if path == "observation" {
            assert!(json["cpu_percent"].is_null());
            assert!(json["rates"]["read_bytes"].is_null());
        }
        if path == "threads" {
            assert!(
                json.as_array()
                    .unwrap()
                    .iter()
                    .all(|t| t["cpu_percent"].is_null())
            );
        }
    }
    assert_eq!(state.monitoring.observer_count(), 0);
    state.stop();
}

#[tokio::test]
async fn target_streams_have_independent_lifetimes_and_histories() {
    let state = Arc::new(AppState::new(Duration::from_millis(100)));
    let app = super::router::router(state.clone(), "127.0.0.1:8080".parse().unwrap());
    let id = super::process::identity(std::process::id() as i32).unwrap();
    let uri = format!("/api/processes/events?{}", query(id));
    let mut a = app
        .clone()
        .oneshot(request(&uri))
        .await
        .unwrap()
        .into_body();
    assert_eq!(next(&mut a).await["history"].as_array().unwrap().len(), 1);
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert!(next(&mut a).await["history"].as_array().unwrap().len() >= 2);
    let mut b = app
        .clone()
        .oneshot(request(&uri))
        .await
        .unwrap()
        .into_body();
    assert_eq!(next(&mut b).await["history"].as_array().unwrap().len(), 1);
    assert_eq!(state.monitoring.observer_count(), 2);
    drop(a);
    assert_eq!(state.monitoring.observer_count(), 1);
    assert_eq!(next(&mut b).await["summary"]["identity"]["pid"], id.pid);
    let mut bodies = vec![b];
    for _ in 1..32 {
        bodies.push(
            app.clone()
                .oneshot(request(&uri))
                .await
                .unwrap()
                .into_body(),
        );
    }
    assert_eq!(
        app.clone().oneshot(request(&uri)).await.unwrap().status(),
        429
    );
    drop(bodies.pop()); // Unpolled streams must release their slot too.
    let response = app.clone().oneshot(request(&uri)).await.unwrap();
    assert_eq!(response.status(), 200);
    bodies.push(response.into_body());
    state.stop();
    assert_eq!(
        app.clone().oneshot(request(&uri)).await.unwrap().status(),
        503
    );
    for body in bodies {
        tokio::time::timeout(Duration::from_secs(3), to_bytes(body, usize::MAX))
            .await
            .unwrap()
            .unwrap();
    }
    assert_eq!(state.monitoring.observer_count(), 0);
    state.join_collectors().unwrap();
}

#[tokio::test]
async fn closing_a_session_stops_its_collector() {
    let state = Arc::new(AppState::new(Duration::from_millis(100)));
    let id = super::process::identity(std::process::id() as i32).unwrap();
    let session = state
        .monitoring
        .observe(id, state.monitoring.reserve().unwrap())
        .unwrap();
    let receiver = session.receiver.clone();
    drop(session);
    tokio::time::timeout(Duration::from_secs(2), async {
        while receiver.has_changed().is_ok() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(state.monitoring.observer_count(), 0);
    state.stop();
    state.join_collectors().unwrap();
}

use super::process::TestTarget as Target;

#[tokio::test]
async fn api_explicit_identity_validation_and_concurrent_snapshots() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let target = Target::new("sleeping");
    let state = Arc::new(AppState::new(Duration::from_secs(1)));
    let app = super::router::router(state, "127.0.0.1:8080".parse().unwrap());
    for path in ["environment", "auxv", "fds", "signals"] {
        for (start, expected) in [
            (target.id.start_time_ticks, 200),
            (target.id.start_time_ticks + 1, 410),
        ] {
            let uri = format!(
                "/api/processes/{path}?pid={}&start_time_ticks={start}",
                target.id.pid
            );
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(uri)
                        .header("host", "127.0.0.1:8080")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), expected);
            assert_eq!(response.headers()["cache-control"], "no-store");
        }
    }
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/api/processes/memory?pid={}&start_time_ticks={}&address=0x{:x}&length=8",
                    target.id.pid, target.id.start_time_ticks, target.address
                ))
                .header("host", "127.0.0.1:8080")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 404);
    // Two callers must not attach to the same tracee concurrently.
    let snapshot_request = || {
        Request::builder()
            .method("POST")
            .uri("/api/processes/snapshot")
            .header("host", "127.0.0.1:8080")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_vec(&target.id).unwrap()))
            .unwrap()
    };
    let (a, b) = tokio::join!(
        app.clone().oneshot(snapshot_request()),
        app.clone().oneshot(snapshot_request()),
    );
    assert_eq!(a.unwrap().status(), 200);
    assert_eq!(b.unwrap().status(), 200);
    target.assert_detached();
}
