use procinsh::{
    process,
    space::{LeaseRequest, Space, topology},
};
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
#[test]
fn leases_are_independent_and_stop_collectors() {
    let s = Arc::new(Space::default());
    assert!(
        serde_json::from_value::<LeaseRequest>(serde_json::json!({
            "selected_process": {"pid": 1, "start_time_ticks": 1}
        }))
        .is_err()
    );
    assert!(serde_json::from_value::<LeaseRequest>(serde_json::json!({"density": 2})).is_err());
    assert!(serde_json::from_value::<LeaseRequest>(serde_json::json!({})).is_ok());
    assert!(
        s.lease(LeaseRequest {
            token: Some("unknown".into()),
        })
        .is_err()
    );
    let a = s.lease(LeaseRequest { token: None }).unwrap();
    let b = s.lease(LeaseRequest { token: None }).unwrap();
    assert_ne!(a.token, b.token);
    let renewed = s
        .lease(LeaseRequest {
            token: Some(a.token.clone()),
        })
        .unwrap();
    assert_eq!(renewed.token, a.token);
    assert_eq!(renewed.expires_in, 30);
    assert!(s.active());
    s.release(&b.token);
    assert!(s.active());
    s.release(&a.token);
    assert!(!s.active());
    s.stop();
    std::thread::sleep(Duration::from_millis(150));
    assert!(s.stopped());
    assert!(process::identity(std::process::id() as i32).is_ok());
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
