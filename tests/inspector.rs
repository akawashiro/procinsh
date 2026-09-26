use procinsh::{
    process::{self, discovery::Discovery, maps, memory, threads},
    state::AppState,
};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::{Arc, OnceLock},
    time::Duration,
};

fn build_targets() {
    static BUILD: OnceLock<()> = OnceLock::new();
    BUILD.get_or_init(|| {
        assert!(
            Command::new("sh")
                .arg("tests/targets/build.sh")
                .status()
                .unwrap()
                .success()
        );
    });
}
struct Target {
    child: Child,
    id: process::ProcessId,
    address: u64,
}
impl Target {
    fn new(name: &str) -> Self {
        build_targets();
        let mut child = Command::new(format!("tests/targets/bin/{name}"))
            .env("PROCINSH_TEST_ENV", "value=with\nline <b>literal</b>")
            .env("PROCINSH_TEST_EMPTY", "")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let address = line
            .split_whitespace()
            .nth(1)
            .and_then(|v| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok())
            .unwrap_or(0);
        let id = process::identity(child.id() as i32).unwrap();
        std::thread::sleep(Duration::from_millis(30));
        Self { child, id, address }
    }
    fn assert_detached(&self) {
        for tid in threads::tids(self.id.pid).unwrap() {
            if let Ok(status) =
                process::procfs::fields(&format!("/proc/{}/task/{tid}/status", self.id.pid))
            {
                assert_eq!(
                    process::procfs::field_u64(&status, "TracerPid"),
                    Some(0),
                    "TID {tid} remained traced"
                );
                assert!(
                    !status["State"].starts_with('t'),
                    "TID {tid} remained in ptrace stop"
                );
            }
        }
    }
}
impl Drop for Target {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn discovery_rates_history_and_process_exit() {
    let mut target = Target::new("busy_loop");
    let mut discovery = Discovery::default();
    assert!(
        discovery
            .collect()
            .unwrap()
            .iter()
            .any(|p| p.identity == target.id)
    );
    let app = Arc::new(AppState::new(Duration::from_millis(100)));
    let session = app.observe(target.id, app.reserve().unwrap()).unwrap();
    let other_id = process::identity(std::process::id() as i32).unwrap();
    let other = app.observe(other_id, app.reserve().unwrap()).unwrap();
    std::thread::sleep(Duration::from_millis(350));
    {
        let t = session.receiver.borrow();
        let o = t.observation.as_ref().unwrap();
        assert!(o.cpu_percent.unwrap() > 0.0);
        assert!(o.rss_bytes > 0);
        assert!(t.history.len() >= 2);
        assert!(o.threads.iter().any(|t| t.tid == target.id.pid));
        assert!(!t.maps.is_empty());
    }
    assert!(
        discovery
            .collect()
            .unwrap()
            .iter()
            .find(|p| p.identity == target.id)
            .unwrap()
            .cpu_percent
            .unwrap()
            > 0.0
    );
    target.child.kill().unwrap();
    target.child.wait().unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert!(session.receiver.borrow().exited);
    assert!(!other.receiver.borrow().exited);
    assert_eq!(other.receiver.borrow().summary.identity, other_id);
    assert!(app.observe(target.id, app.reserve().unwrap()).is_err());
    app.stop();
    drop(session);
    drop(other);
    app.join_collectors().unwrap();
}

#[test]
fn memory_partial_reads_and_map_statistics() {
    let target = Target::new("mmap_test");
    let data = memory::read(target.id, target.address, 25).unwrap();
    assert!(data.bytes.starts_with(b"procinsh mmap fixture"));
    let page = process::procfs::page_size();
    let data = memory::read(target.id, target.address + page - 8, 16).unwrap();
    assert!(data.partial);
    assert_eq!(data.bytes.len(), 8);
    assert!(memory::read(target.id, target.address + page, 16).is_err());
    let maps = maps::read(target.id.pid, true).unwrap();
    assert!(
        maps.iter()
            .any(|m| m.contains(target.address) && m.rss_bytes.is_some())
    );
}

#[test]
fn environment_and_auxv_read_child_metadata_and_reject_stale_identity() {
    let mut target = Target::new("sleeping");
    let environment = process::details::environment(target.id).unwrap();
    let entry = environment
        .entries
        .iter()
        .find(|e| e.name == "PROCINSH_TEST_ENV")
        .unwrap();
    assert_eq!(
        entry.value.as_deref(),
        Some("value=with\nline <b>literal</b>")
    );
    assert_eq!(
        environment
            .entries
            .iter()
            .find(|e| e.name == "PROCINSH_TEST_EMPTY")
            .unwrap()
            .value
            .as_deref(),
        Some("")
    );
    let auxv = process::details::auxv(target.id).unwrap();
    assert_eq!(auxv.word_bits, 64);
    assert_eq!(
        auxv.entries
            .iter()
            .find(|e| e.name == "AT_PAGESZ")
            .unwrap()
            .value,
        process::procfs::page_size()
    );
    assert!(
        auxv.entries
            .iter()
            .find(|e| e.name == "AT_EXECFN")
            .unwrap()
            .text
            .as_deref()
            .unwrap()
            .ends_with("/sleeping")
    );
    assert_eq!(auxv.entries.last().unwrap().name, "AT_NULL");
    let json = serde_json::to_value(&auxv).unwrap();
    assert!(
        json["entries"][0]["value"]
            .as_str()
            .unwrap()
            .starts_with("0x")
    );
    let stale = process::ProcessId {
        start_time_ticks: target.id.start_time_ticks + 1,
        ..target.id
    };
    assert!(process::details::environment(stale).is_err());
    assert!(process::details::auxv(stale).is_err());
    target.child.kill().unwrap();
    target.child.wait().unwrap();
    assert!(process::details::environment(target.id).is_err());
    assert!(process::details::auxv(target.id).is_err());
}

#[test]
fn pipe_unix_tcp_udp_peers_are_distinct_from_shared_descriptors() {
    let target = Target::new("ipc");
    let child: i32 = std::fs::read_to_string(format!(
        "/proc/{}/task/{}/children",
        target.id.pid, target.id.pid
    ))
    .unwrap()
    .split_whitespace()
    .next()
    .unwrap()
    .parse()
    .unwrap();
    let child_id = process::identity(child).unwrap();
    let details = process::fds::read(target.id).unwrap();
    for fd in [60, 61, 62, 64] {
        let entry = details.entries.iter().find(|e| e.fd == fd).unwrap();
        assert!(
            entry
                .peers
                .iter()
                .any(|p| p.process_id == child_id && p.fd == fd),
            "missing peer for FD {fd}: {entry:?}; {:?}",
            details.warnings
        );
    }
    let unix = details.entries.iter().find(|e| e.fd == 61).unwrap();
    assert!(unix.peer_inode.is_some());
    assert!(
        unix.holders
            .iter()
            .any(|p| p.process_id == child_id && p.fd == 63)
    );
    assert!(!unix.peers.iter().any(|p| p.fd == 63));
    let listener = details.entries.iter().find(|e| e.fd == 65).unwrap();
    assert_eq!(listener.state.as_deref(), Some("LISTEN"));
    assert!(listener.peers.is_empty());
    let stale = process::ProcessId {
        start_time_ticks: target.id.start_time_ticks + 1,
        ..target.id
    };
    assert!(process::fds::read(stale).is_err());
    unsafe {
        libc::kill(child, libc::SIGTERM);
    }
    for _ in 0..100 {
        if process::check_identity(child_id).is_err() {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let after = process::fds::read(target.id).unwrap();
    assert!(
        !after
            .entries
            .iter()
            .flat_map(|e| &e.peers)
            .any(|p| p.process_id == child_id)
    );
}

#[tokio::test]
async fn api_explicit_identity_validation_and_memory_limits() {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;
    let target = Target::new("sleeping");
    let state = Arc::new(AppState::new(Duration::from_secs(1)));
    let app = procinsh::server::router(state, "127.0.0.1:8080".parse().unwrap());
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
    for (start, length, expected) in [
        (target.id.start_time_ticks, 8, 200),
        (target.id.start_time_ticks + 1, 8, 410),
        (target.id.start_time_ticks, 65537, 400),
    ] {
        let uri = format!(
            "/api/processes/memory?pid={}&start_time_ticks={start}&address=0x{:x}&length={length}",
            target.id.pid, target.address
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
    }
    for (start, address, expected) in [
        (
            target.id.start_time_ticks,
            format!("0x{:x}", target.address),
            200,
        ),
        (target.id.start_time_ticks + 1, "0x0".into(), 410),
        (target.id.start_time_ticks, "invalid".into(), 400),
    ] {
        let response = app.clone().oneshot(Request::builder()
            .uri(format!("/api/processes/disassembly?pid={}&start_time_ticks={start}&address={address}", target.id.pid))
            .header("host", "127.0.0.1:8080").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status().as_u16(), expected);
    }
    target.assert_detached();
}
