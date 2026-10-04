use super::{self as process, maps, memory, test_support::Target};
use std::time::Duration;

#[test]
fn memory_partial_reads_and_map_statistics() {
    let target = Target::new("mmap_test");
    let data = memory::read_raw(target.id.pid, target.address, 25).unwrap();
    assert!(data.starts_with(b"procinsh mmap fixture"));
    let page = process::procfs::page_size();
    let data = memory::read_raw(target.id.pid, target.address + page - 8, 16).unwrap();
    assert_eq!(data.len(), 8);
    assert!(memory::read_raw(target.id.pid, target.address + page, 16).is_err());
    let maps = maps::read_smaps(target.id.pid).unwrap();
    assert!(
        maps.iter()
            .any(|m| m.mapping.contains(target.address) && m.rss_bytes.is_some())
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
    assert_eq!(
        listener.state,
        Some(crate::http_server::socket_types::SocketState::Listen)
    );
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
