use super as system_snapshot;
use std::{
    io::{BufRead, BufReader},
    process::{Command, Stdio},
};
struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[test]
fn snapshot_finds_pipe_and_unix_peers_and_bounds_work() {
    super::super::super::test_support::build_targets();
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
    let peer_pid: i32 = line.split_whitespace().nth(2).unwrap().parse().unwrap();
    let snapshot = system_snapshot::collect(&mut Default::default());
    assert!(snapshot.processes.iter().any(|n| n.identity.pid == parent));
    assert_eq!(
        snapshot
            .processes
            .iter()
            .find(|n| n.identity.pid == peer_pid)
            .and_then(|n| n.parent_id)
            .map(|id| id.pid),
        Some(parent)
    );
    assert!(snapshot.fd_relations.iter().any(|e| {
        !e.shared
            && ((e.endpoint.process_id.pid == parent
                && e.peer
                    .as_ref()
                    .is_some_and(|peer| peer.process_id.pid == peer_pid))
                || (e.endpoint.process_id.pid == peer_pid
                    && e.peer
                        .as_ref()
                        .is_some_and(|peer| peer.process_id.pid == parent)))
    }));
    assert!(snapshot.inspected_fds <= 100_000);
    let json = serde_json::to_value(&snapshot).unwrap();
    let relations = json["fd_relations"].as_array().unwrap();
    assert!(!relations.is_empty());
    for relation in relations {
        assert!(relation["endpoint"]["process_id"].is_object());
        assert!(relation.get("peer").is_some());
        assert!(relation.get("a").is_none());
        assert!(relation.get("b").is_none());
    }
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
    let data = system_snapshot::collect(&mut Default::default());
    for pid in [std::process::id() as i32, child.0.id() as i32] {
        let relation = data
            .fd_relations
            .iter()
            .find(|e| {
                e.endpoint.process_id.pid == pid
                    && e.peer.is_none()
                    && e.socket
                        .as_ref()
                        .is_some_and(|s| s.remote == Some(socket.peer_addr().unwrap()))
            })
            .expect("shared socket retains a network destination for each owner");
        let info = relation.socket.as_ref().unwrap();
        assert!(info.network_peer);
        assert_eq!(info.local, Some(socket.local_addr().unwrap()));
        assert_eq!(info.protocol, "UDP");
    }
    assert!(data.fd_relations.iter().any(|e| {
        e.shared
            && e.peer.as_ref().is_some_and(|peer| {
                peer.process_id.pid == child.0.id() as i32
                    || e.endpoint.process_id.pid == child.0.id() as i32
            })
    }));
}
