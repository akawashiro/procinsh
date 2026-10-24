use super::{self as process, threads};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    time::Duration,
};
pub(in crate::privileged_helper) struct Target {
    pub(in crate::privileged_helper) child: Child,
    pub(in crate::privileged_helper) id: process::ProcessId,
    pub(in crate::privileged_helper) address: u64,
}
impl Target {
    pub(in crate::privileged_helper) fn new(name: &str) -> Self {
        super::super::test_support::build_targets();
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

    pub(in crate::privileged_helper) fn assert_detached(&self) {
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

/// Two UNIX endpoints in a fresh network namespace, plus a host socket inherited
/// by the parent. The host endpoint remains owned by the test process as well.
pub(in crate::privileged_helper) struct NetworkNamespaceTarget {
    pub(in crate::privileged_helper) target: Target,
    pub(in crate::privileged_helper) peer_id: process::ProcessId,
    pub(in crate::privileged_helper) inherited: std::os::unix::net::UnixStream,
    _inherited_peer: std::os::unix::net::UnixStream,
}

impl NetworkNamespaceTarget {
    pub(in crate::privileged_helper) fn new(separate: bool) -> Self {
        use std::os::{
            fd::OwnedFd,
            unix::{net::UnixStream, process::CommandExt},
        };
        super::super::test_support::build_targets();
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
        let id = process::identity(child.id() as i32).unwrap();
        let peer_id =
            process::identity(line.split_whitespace().nth(1).unwrap().parse().unwrap()).unwrap();
        Self {
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
}
