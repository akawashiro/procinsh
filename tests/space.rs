use procinsh::space::topology;
use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
    sync::Arc,
    time::Duration,
};
struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
fn topology_finds_pipe_and_unix_peers_and_bounds_work() {
    let mut child = Child(
        Command::new("tests/targets/bin/ipc")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut line = String::new();
    BufReader::new(child.0.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let parent = child.0.id() as i32;
    let peer: i32 = line.split_whitespace().nth(2).unwrap().parse().unwrap();
    let topology = topology::collect(&mut Default::default());
    assert!(topology.nodes.iter().any(|n| n.identity.pid == parent));
    assert_eq!(
        topology
            .nodes
            .iter()
            .find(|n| n.identity.pid == peer)
            .and_then(|n| n.parent_id)
            .map(|id| id.pid),
        Some(parent)
    );
    assert!(topology.edges.iter().any(|e| !e.shared
        && ((e.a.process_id.pid == parent
            && e.b.as_ref().is_some_and(|b| b.process_id.pid == peer))
            || (e.a.process_id.pid == peer
                && e.b.as_ref().is_some_and(|b| b.process_id.pid == parent)))));
    assert!(topology.inspected_fds <= 100_000);
}
#[tokio::test]
async fn sse_connections_own_viewer_lifetimes() {
    use axum::{
        body::{Body, HttpBody},
        http::{Request, StatusCode},
    };
    use procinsh::state::AppState;
    use tower::ServiceExt;
    let state = Arc::new(AppState::new(Duration::from_secs(1)));
    let app = procinsh::server::router(state.clone(), "127.0.0.1:8080".parse().unwrap());
    let request = |path: &str| {
        Request::builder()
            .uri(path)
            .header("host", "127.0.0.1:8080")
            .body(Body::empty())
            .unwrap()
    };
    for path in ["/api/system/status", "/api/system/topology"] {
        assert_eq!(
            app.clone().oneshot(request(path)).await.unwrap().status(),
            StatusCode::OK
        );
        assert!(!state.space.active());
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
    assert!(state.space.active());
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
            .contains("event: topology")
    );
    responses.clear();
    assert!(state.space.active());
    drop(body);
    assert!(!state.space.active());
    let response = app
        .clone()
        .oneshot(request("/api/system/events"))
        .await
        .unwrap();
    assert!(state.space.active());
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
    assert!(!state.space.active());
}

#[test]
fn network_destination_survives_shared_socket_ownership() {
    let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    socket.connect("127.0.0.1:54321").unwrap();
    let inherited: std::os::fd::OwnedFd = socket.try_clone().unwrap().into();
    let child = Child(
        Command::new("sleep")
            .arg("30")
            .stdin(Stdio::from(inherited))
            .spawn()
            .unwrap(),
    );
    let data = topology::collect(&mut Default::default());
    for pid in [std::process::id() as i32, child.0.id() as i32] {
        let edge = data
            .edges
            .iter()
            .find(|e| {
                e.a.process_id.pid == pid
                    && e.b.is_none()
                    && e.socket
                        .as_ref()
                        .is_some_and(|s| s.remote == Some(socket.peer_addr().unwrap()))
            })
            .expect("shared socket retains a network destination for each owner");
        let info = edge.socket.as_ref().unwrap();
        assert!(info.network_peer);
        assert_eq!(info.local, Some(socket.local_addr().unwrap()));
        assert_eq!(info.protocol, "UDP");
    }
    assert!(data.edges.iter().any(|e| {
        e.shared
            && e.b.as_ref().is_some_and(|b| {
                b.process_id.pid == child.0.id() as i32 || e.a.process_id.pid == child.0.id() as i32
            })
    }));
}
