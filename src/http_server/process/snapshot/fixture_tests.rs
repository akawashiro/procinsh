use super as snapshot;
use crate::http_server::process::snapshot::symbol::ElfCache;
use crate::http_server::process::{self as process, memory, test_support::Target, threads};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

#[test]
fn coherent_snapshot_unwinds_and_resolves_pie_source() {
    let target = Target::new("recursive");
    let symbols = Arc::new(Mutex::new(ElfCache::default()));
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
            snapshot::capture(target.id, Arc::new(Mutex::new(ElfCache::default()))).unwrap();
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
    let result = snapshot::capture(target.id, Arc::new(Mutex::new(ElfCache::default())));
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
            snapshot::capture(target.id, Arc::new(Mutex::new(ElfCache::default()))).unwrap();
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
    snapshot::capture(target.id, Arc::new(Mutex::new(ElfCache::default()))).unwrap();
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
            Arc::new(Mutex::new(ElfCache::default()))
        )
        .is_err()
    );
    assert!(memory::read(id, target.address, 10).is_ok());
}
