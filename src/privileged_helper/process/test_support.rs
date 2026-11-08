use super::threads;
use crate::privileged_helper::process;
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    time::Duration,
};
pub(in crate::privileged_helper) struct Target {
    pub(in crate::privileged_helper) child: Child,
    pub(in crate::privileged_helper) id: crate::ipc::schema::ProcessId,
    pub(in crate::privileged_helper) address: u64,
}
pub(in crate::privileged_helper) fn create_target(name: &str) -> Target {
    crate::test_support::build_targets();
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
    let id = process::read_process_identity(child.id() as i32).unwrap();
    std::thread::sleep(Duration::from_millis(30));
    Target { child, id, address }
}

pub(in crate::privileged_helper) fn assert_detached_target(state: &Target) {
    for tid in threads::collect_thread_ids(state.id.pid).unwrap() {
        if let Ok(status) = process::procfs::read_procfs_fields(&format!(
            "/proc/{}/task/{tid}/status",
            state.id.pid
        )) {
            assert_eq!(
                process::procfs::parse_procfs_u64_field(&status, "TracerPid"),
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
impl Drop for Target {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Two UNIX endpoints in a fresh network namespace, plus a host socket inherited
/// by the parent. The host endpoint remains owned by the test process as well.
pub(in crate::privileged_helper) struct NetworkNamespaceTarget {
    pub(in crate::privileged_helper) target: Target,
    pub(in crate::privileged_helper) peer_id: crate::ipc::schema::ProcessId,
    pub(in crate::privileged_helper) inherited: std::os::unix::net::UnixStream,
    _inherited_peer: std::os::unix::net::UnixStream,
}

pub(in crate::privileged_helper) fn create_network_namespace_target(
    separate: bool,
) -> NetworkNamespaceTarget {
    use std::os::{
        fd::OwnedFd,
        unix::{net::UnixStream, process::CommandExt},
    };
    crate::test_support::build_targets();
    let (inherited, inherited_peer) = UnixStream::pair().unwrap();
    let stdin: OwnedFd = inherited.try_clone().unwrap().into();
    let mut command = Command::new("tests/targets/bin/netns_ipc");
    command
        .arg("--allow-inspector")
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::piped());
    if separate {
        // Only an async-signal-safe syscall runs between fork and exec.
        unsafe {
            command.pre_exec(|| {
                if libc::unshare(libc::CLONE_NEWNET) == 0 {
                    Ok(())
                } else {
                    Err(std::io::Error::last_os_error())
                }
            });
        }
    }
    let mut child = command
        .spawn()
        .expect("network namespaces are required in the Docker test container");
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let id = process::read_process_identity(child.id() as i32).unwrap();
    let peer_id =
        process::read_process_identity(line.split_whitespace().nth(1).unwrap().parse().unwrap())
            .unwrap();
    NetworkNamespaceTarget {
        target: Target {
            child,
            id,
            address: 0,
        },
        peer_id,
        inherited,
        _inherited_peer: inherited_peer,
    }
}
