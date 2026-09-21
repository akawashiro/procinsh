use procinsh::{
    process::{self, discovery::Discovery, maps, memory, threads},
    snapshot,
    state::AppState,
    symbol::Symbolizer,
};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, OnceLock},
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
    app.select(target.id).unwrap();
    let collector = app.start_collector();
    std::thread::sleep(Duration::from_millis(350));
    {
        let inner = app.lock();
        let t = inner.target.as_ref().unwrap();
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
    assert!(app.lock().target.as_ref().unwrap().exited);
    assert!(app.select(target.id).is_err());
    app.stop();
    collector.join().unwrap();
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
fn coherent_snapshot_unwinds_and_resolves_pie_source() {
    let target = Target::new("recursive");
    let symbols = Arc::new(Mutex::new(Symbolizer::default()));
    let snapshot = snapshot::capture(target.id, symbols.clone()).unwrap();
    target.assert_detached();
    let thread = snapshot
        .threads
        .iter()
        .find(|t| t.tid == target.id.pid)
        .unwrap();
    assert_eq!(thread.registers.len(), 18);
    let code = thread
        .disassembly
        .as_ref()
        .expect("captured instruction bytes");
    let rip = thread.registers.iter().find(|r| r.name == "RIP").unwrap();
    assert_eq!(code.address, rip.value);
    assert!(!code.instructions.is_empty(), "{code:?}");
    assert_eq!(code.instructions[0].address, rip.value);
    assert!(code.instructions[0].current);
    assert!(code.bytes.starts_with(&code.instructions[0].bytes));
    assert!(code.instructions.len() <= 32);
    assert!(code.bytes.len() <= 256);
    let frames = &thread.call_stack;
    for name in ["baz", "bar", "foo", "main"] {
        let frame = frames
            .iter()
            .find(|f| f.symbol.as_deref() == Some(name))
            .unwrap_or_else(|| panic!("missing {name}: {frames:#?}"));
        assert!(
            frame
                .source_file
                .as_deref()
                .unwrap()
                .ends_with("recursive.c")
        );
        assert!(frame.line.unwrap() > 0);
    }
    let rsp = thread.registers.iter().find(|r| r.name == "RSP").unwrap();
    assert!(rsp.mapping.is_some());
    // Repeated capture exercises the ELF/DWARF cache after the tracee resumes.
    assert!(
        !snapshot::capture(target.id, symbols)
            .unwrap()
            .threads
            .is_empty()
    );
    target.assert_detached();
}

#[test]
fn symbols_support_non_pie_and_missing_debug_information() {
    for name in ["recursive_nopie", "recursive_nodebug"] {
        let target = Target::new(name);
        let snapshot =
            snapshot::capture(target.id, Arc::new(Mutex::new(Symbolizer::default()))).unwrap();
        let frame = snapshot.threads[0]
            .call_stack
            .iter()
            .find(|f| f.symbol.as_deref() == Some("foo"))
            .expect("ELF symbol foo");
        assert_eq!(frame.source_file.is_some(), name == "recursive_nopie");
        target.assert_detached();
    }
}

#[test]
fn partial_attach_failure_releases_previously_attached_threads() {
    let target = Target::new("threads");
    let tid = threads::tids(target.id.pid).unwrap()[1];
    let (attached_tx, attached_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let other_tracer = std::thread::spawn(move || {
        unsafe {
            assert_eq!(libc::ptrace(libc::PTRACE_SEIZE, tid, 0usize, 0usize), 0);
        }
        attached_tx.send(()).unwrap();
        release_rx.recv().unwrap();
        unsafe {
            assert_eq!(libc::ptrace(libc::PTRACE_INTERRUPT, tid, 0usize, 0usize), 0);
            let mut status = 0;
            assert_eq!(libc::waitpid(tid, &mut status, libc::__WALL), tid);
            assert_eq!(libc::ptrace(libc::PTRACE_DETACH, tid, 0usize, 0usize), 0);
        }
    });
    attached_rx.recv().unwrap();
    let result = snapshot::capture(target.id, Arc::new(Mutex::new(Symbolizer::default())));
    release_tx.send(()).unwrap();
    other_tracer.join().unwrap();
    assert!(result.is_err());
    target.assert_detached();
}

#[test]
fn snapshot_handles_thread_churn_and_preserves_job_control_stop() {
    let target = Target::new("threads");
    for _ in 0..3 {
        let result =
            snapshot::capture(target.id, Arc::new(Mutex::new(Symbolizer::default()))).unwrap();
        assert!(result.threads.len() >= 6);
        target.assert_detached();
    }
    unsafe {
        libc::kill(target.id.pid, libc::SIGSTOP);
    }
    for _ in 0..100 {
        if process::procfs::read_stat(&format!("/proc/{}/stat", target.id.pid))
            .unwrap()
            .state
            == "T"
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    snapshot::capture(target.id, Arc::new(Mutex::new(Symbolizer::default()))).unwrap();
    target.assert_detached();
    assert_eq!(
        process::procfs::read_stat(&format!("/proc/{}/stat", target.id.pid))
            .unwrap()
            .state,
        "T"
    );
    unsafe {
        libc::kill(target.id.pid, libc::SIGCONT);
    }
}

#[test]
fn guard_cleans_up_on_error_and_panic() {
    let target = Target::new("sleeping");
    let id = target.id;
    let result = std::thread::spawn(move || -> anyhow::Result<()> {
        let _guard = snapshot::ptrace::SnapshotGuard::capture(id)?;
        anyhow::bail!("injected failure after all threads stop")
    })
    .join()
    .unwrap();
    assert!(result.is_err());
    target.assert_detached();
    let result = std::thread::spawn(move || {
        let _guard = snapshot::ptrace::SnapshotGuard::capture(id).unwrap();
        panic!("injected panic after all threads stop");
    })
    .join();
    assert!(result.is_err());
    target.assert_detached();
    assert!(
        snapshot::capture(
            process::ProcessId {
                start_time_ticks: id.start_time_ticks + 1,
                ..id
            },
            Arc::new(Mutex::new(Symbolizer::default()))
        )
        .is_err()
    );
    assert!(memory::read(id, target.address, 10).is_ok());
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
async fn api_selection_identity_validation_and_memory_limits() {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;
    let target = Target::new("sleeping");
    let state = Arc::new(AppState::new(Duration::from_secs(1)));
    let app = procinsh::server::router(state, "127.0.0.1:8080".parse().unwrap());
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/target")
                .header("host", "127.0.0.1:8080")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&target.id).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let body: serde_json::Value =
        serde_json::from_slice(&to_bytes(response.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(body["summary"]["identity"]["pid"], target.id.pid);
    for path in ["environment", "auxv", "fds", "signals"] {
        for (start, expected) in [
            (target.id.start_time_ticks, 200),
            (target.id.start_time_ticks + 1, 409),
        ] {
            let uri = format!(
                "/api/target/{path}?pid={}&start_time_ticks={start}",
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
        (target.id.start_time_ticks + 1, 8, 409),
        (target.id.start_time_ticks, 65537, 400),
    ] {
        let uri = format!(
            "/api/target/memory?pid={}&start_time_ticks={start}&address=0x{:x}&length={length}",
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
}
