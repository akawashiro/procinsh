//! Run explicitly on a Linux host with perf access: cargo test --test perf_live -- --ignored --test-threads=1
//! Unavailable perf is a failure here, never a successful skip.
use procinsh::{
    perf::{Config, PerfManager},
    process,
    symbol::Symbolizer,
};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
struct Target(Child);
impl Target {
    fn new(name: &str) -> Self {
        let mut c = Command::new(format!("tests/targets/bin/{name}"))
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(c.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        Self(c)
    }
    fn id(&self) -> process::ProcessId {
        process::identity(self.0.id() as i32).unwrap()
    }
}
impl Drop for Target {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn wait(mut check: impl FnMut() -> bool) {
    let start = Instant::now();
    while !check() {
        assert!(
            start.elapsed() < Duration::from_secs(8),
            "timed out waiting for perf"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
#[test]
#[ignore = "requires perf_event_open access; dedicated live suite"]
fn cpu_samples_registers_callchain_sharing_and_release() {
    let t = Target::new("busy_loop");
    let id = t.id();
    let manager = Arc::new(PerfManager::new(Config::default()));
    let symbols = Arc::new(Mutex::new(Symbolizer::default()));
    let perf_fds = || {
        std::fs::read_dir("/proc/self/fd")
            .unwrap()
            .flatten()
            .filter(|e| {
                std::fs::read_link(e.path())
                    .is_ok_and(|p| p.to_string_lossy().contains("perf_event"))
            })
            .count()
    };
    let before_fds = perf_fds();
    let a = manager.subscribe(id, symbols.clone()).unwrap();
    let b = manager.subscribe(id, symbols.clone()).unwrap();
    assert_eq!(manager.sampler_count(), 1);
    wait(|| !a.view().threads.is_empty());
    let data = a.view();
    assert_eq!(data.status, "active", "{:?}", data.warnings);
    let s = &data.threads[0];
    assert_ne!(s.ip, 0);
    assert_eq!(s.registers.len(), 18);
    assert!(!s.call_stack.is_empty(), "{s:?}");
    assert!(s.sample_age_ms < 3000);
    assert_eq!(
        process::procfs::field_u64(
            &process::procfs::fields(&format!("/proc/{}/status", id.pid)).unwrap(),
            "TracerPid"
        ),
        Some(0)
    );
    let stale = process::ProcessId {
        start_time_ticks: id.start_time_ticks + 1,
        ..id
    };
    assert!(manager.subscribe(stale, symbols).is_err());
    drop(a);
    assert_eq!(manager.sampler_count(), 1);
    drop(b);
    assert_eq!(manager.sampler_count(), 0);
    assert_eq!(
        perf_fds(),
        before_fds,
        "last subscription must close perf FDs"
    );
}
#[test]
#[ignore = "requires perf_event_open access; dedicated live suite"]
fn sleeping_churn_exit_and_shutdown() {
    let sleeping = Target::new("sleeping");
    let manager = Arc::new(PerfManager::new(Config::default()));
    let symbols = Arc::new(Mutex::new(Symbolizer::default()));
    let session = manager.subscribe(sleeping.id(), symbols.clone()).unwrap();
    std::thread::sleep(Duration::from_secs(2));
    assert_eq!(
        session.view().status,
        "active",
        "{:?}",
        session.view().warnings
    );
    assert!(
        session.view().history.len() < 10,
        "sleep must not fabricate 49 Hz samples"
    );
    let mut churn = Target::new("threads");
    let other = manager.subscribe(churn.id(), symbols).unwrap();
    wait(|| other.view().monitored_threads >= 6);
    std::thread::sleep(Duration::from_secs(2));
    churn.0.kill().unwrap();
    churn.0.wait().unwrap();
    wait(|| other.view().status == "stopped");
    manager.stop();
    manager.join();
    assert_eq!(manager.sampler_count(), 0);
    drop(other);
    drop(session);
}

#[test]
#[ignore = "requires perf_event_open access; dedicated live suite"]
fn exec_stops_and_dynamic_code_keeps_raw_addresses() {
    let mut child = Command::new("tests/targets/bin/perf_workload")
        .args(["--allow-inspector", "2", "jit"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    output.read_line(&mut line).unwrap();
    let target = Target(child);
    let manager = Arc::new(PerfManager::new(Config::default()));
    let session = manager
        .subscribe(target.id(), Arc::new(Mutex::new(Symbolizer::default())))
        .unwrap();
    wait(|| session.view().threads.len() >= 2);
    assert!(session.view().threads.iter().all(|s| s.ip != 0));
    unsafe {
        libc::kill(target.id().pid, libc::SIGUSR1);
    }
    wait(|| session.view().status == "stopped");
    assert!(process::check_identity(target.id()).is_ok());
    drop(session);
    assert_eq!(manager.sampler_count(), 0);
}

#[test]
#[ignore = "requires perf_event_open access; dedicated live suite"]
fn callchain_symbols_pie_nonpie_and_missing_debug_info() {
    for name in ["recursive", "recursive_nopie", "recursive_nodebug"] {
        let t = Target::new(name);
        let manager = Arc::new(PerfManager::new(Config::default()));
        let session = manager
            .subscribe(t.id(), Arc::new(Mutex::new(Symbolizer::default())))
            .unwrap();
        wait(|| {
            session.view().threads.iter().any(|s| {
                s.call_stack
                    .iter()
                    .any(|f| f.symbol.as_deref() == Some("foo"))
            })
        });
        let data = session.view();
        let frame = data.threads[0]
            .call_stack
            .iter()
            .find(|f| f.symbol.as_deref() == Some("foo"))
            .unwrap();
        assert_eq!(frame.source_file.is_some(), name != "recursive_nodebug");
    }
}

#[test]
#[ignore = "requires perf_event_open access; dedicated live suite"]
fn thread_limit_is_partial_and_keeps_proc_available() {
    let mut child = Command::new("tests/targets/bin/perf_workload")
        .args(["--allow-inspector", "128", "sleep"])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let target = Target(child);
    let manager = Arc::new(PerfManager::new(Config::default()));
    let session = manager
        .subscribe(target.id(), Arc::new(Mutex::new(Symbolizer::default())))
        .unwrap();
    wait(|| session.view().thread_limit_reached);
    let data = session.view();
    assert_eq!(data.status, "partial");
    assert_eq!(data.monitored_threads, 128);
    assert_eq!(process::threads::tids(target.id().pid).unwrap().len(), 129);
    assert!(
        !process::maps::read(target.id().pid, false)
            .unwrap()
            .is_empty()
    );
}
