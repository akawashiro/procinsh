use crate::http_server::process::{self, ProcessId, threads};
use anyhow::{Result, bail, ensure};
use std::time::{Duration, Instant};

struct Tracee {
    tid: i32,
    stopped: bool,
    signal: i32,
    gone: bool,
}
pub(super) struct SnapshotGuard {
    tracees: Vec<Tracee>,
}

fn ptrace(request: libc::c_uint, tid: i32, data: usize) -> std::io::Result<()> {
    // SAFETY: requests here accept a null address; the sole data pointer is GETREGS' valid output.
    if unsafe {
        libc::ptrace(
            request,
            tid,
            std::ptr::null_mut::<libc::c_void>(),
            data as *mut libc::c_void,
        )
    } == -1
    {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn gone(e: &std::io::Error) -> bool {
    matches!(e.raw_os_error(), Some(libc::ESRCH) | Some(libc::ECHILD))
}

fn thread_exited(pid: i32, tid: i32) -> bool {
    match process::procfs::read_stat(&format!("/proc/{pid}/task/{tid}/stat")) {
        Ok(stat) => matches!(stat.state.as_str(), "Z" | "X" | "x"),
        Err(error) => error.downcast_ref::<std::io::Error>().is_some_and(|error| {
            matches!(error.raw_os_error(), Some(libc::ENOENT) | Some(libc::ESRCH))
        }),
    }
}

fn seize(pid: i32, tid: i32) -> std::io::Result<bool> {
    match ptrace(libc::PTRACE_SEIZE, tid, 0) {
        Ok(()) => Ok(true),
        Err(error) if gone(&error) => Ok(false),
        // Linux ptrace_attach also returns EPERM for tasks with exit_state set.
        // Only ignore it when procfs confirms exit; a live task may belong to
        // another tracer or require permissions we do not have.
        Err(error) if error.raw_os_error() == Some(libc::EPERM) && thread_exited(pid, tid) => {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}

impl Tracee {
    fn wait(&mut self, deadline: Instant) -> Result<()> {
        loop {
            let mut status = 0;
            // SAFETY: status is writable; __WALL permits waiting for individual traced threads.
            let result =
                unsafe { libc::waitpid(self.tid, &mut status, libc::__WALL | libc::WNOHANG) };
            if result == self.tid {
                if libc::WIFSTOPPED(status) {
                    self.stopped = true;
                    // Preserve real signal delivery stops and pre-existing group stops.
                    let event = status >> 16;
                    let signal = libc::WSTOPSIG(status);
                    self.signal = if event == libc::PTRACE_EVENT_STOP && signal == libc::SIGTRAP {
                        0
                    } else {
                        signal
                    };
                    return Ok(());
                }
                if libc::WIFEXITED(status) || libc::WIFSIGNALED(status) {
                    self.gone = true;
                    return Ok(());
                }
            } else if result < 0 {
                let e = std::io::Error::last_os_error();
                if gone(&e) {
                    self.gone = true;
                    return Ok(());
                }
                if e.raw_os_error() != Some(libc::EINTR) {
                    return Err(e.into());
                }
            }
            ensure!(
                Instant::now() < deadline,
                "timed out waiting for TID {} to stop",
                self.tid
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
impl SnapshotGuard {
    pub(super) fn capture(id: ProcessId) -> Result<Self> {
        ensure!(
            id.pid != std::process::id() as i32,
            "cannot snapshot the inspector itself (would stop the HTTP server)"
        );
        process::check_identity(id)?;
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut guard = Self {
            tracees: Vec::new(),
        };
        for _ in 0..16 {
            let current = threads::tids(id.pid)?;
            ensure!(current.len() <= 4096, "snapshot exceeds 4096-thread limit");
            for tid in current {
                if guard.tracees.iter().any(|t| t.tid == tid && !t.gone) {
                    continue;
                }
                match seize(id.pid, tid) {
                    Ok(true) => guard.tracees.push(Tracee {
                        tid,
                        stopped: false,
                        signal: 0,
                        gone: false,
                    }),
                    Ok(false) => continue,
                    Err(e) => bail!(process::permission_help("PTRACE_SEIZE", e)),
                }
            }
            for t in guard.tracees.iter_mut().filter(|t| !t.stopped && !t.gone) {
                if let Err(e) = ptrace(libc::PTRACE_INTERRUPT, t.tid, 0) {
                    if gone(&e) {
                        t.gone = true;
                    } else {
                        return Err(e.into());
                    }
                }
            }
            for t in guard.tracees.iter_mut().filter(|t| !t.stopped && !t.gone) {
                t.wait(deadline)?;
            }
            process::check_identity(id)?;
            let current = threads::tids(id.pid)?;
            if current.iter().all(|tid| {
                guard
                    .tracees
                    .iter()
                    .any(|t| t.tid == *tid && t.stopped && !t.gone)
                    || thread_exited(id.pid, *tid)
            }) {
                return Ok(guard);
            }
            ensure!(Instant::now() < deadline, "thread set did not stabilize");
        }
        bail!("thread set did not stabilize after 16 passes")
    }

    pub(super) fn tids(&self) -> Vec<i32> {
        self.tracees
            .iter()
            .filter(|t| t.stopped && !t.gone)
            .map(|t| t.tid)
            .collect()
    }

    pub(super) fn registers(&self, tid: i32) -> Result<libc::user_regs_struct> {
        let mut regs: libc::user_regs_struct = unsafe { std::mem::zeroed() };
        ptrace(libc::PTRACE_GETREGS, tid, &mut regs as *mut _ as usize)?;
        Ok(regs)
    }
}

impl Drop for SnapshotGuard {
    fn drop(&mut self) {
        let deadline = Instant::now() + Duration::from_millis(250);
        for t in self.tracees.iter_mut().rev().filter(|t| !t.gone) {
            if !t.stopped {
                let _ = ptrace(libc::PTRACE_INTERRUPT, t.tid, 0);
                let _ = t.wait(deadline);
            }
            if t.stopped {
                let _ = ptrace(libc::PTRACE_DETACH, t.tid, t.signal as usize);
            }
        }
        // Capture ALWAYS runs on a dedicated, short-lived OS thread. If a task was
        // uninterruptible or DETACH failed, exit of that tracer thread lets Linux
        // detach remaining tracees. Never use EXITKILL or SIGCONT (which would alter
        // an existing job-control stop), and never leave a pooled tracer alive.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seize_skips_confirmed_exit_but_preserves_live_permission_errors() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        let pid = child.id() as i32;
        child.kill().unwrap();
        // Leave a waitable zombie: unlike a reaped task, it makes SEIZE return
        // EPERM, reproducing the enumeration/attach race without timing sleeps.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let waited = unsafe {
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOWAIT,
            )
        };
        let raw = ptrace(libc::PTRACE_SEIZE, pid, 0);
        let exited = thread_exited(pid, pid);
        let attached = seize(pid, pid);
        child.wait().unwrap();
        assert_eq!(waited, 0);
        assert_eq!(raw.unwrap_err().raw_os_error(), Some(libc::EPERM));
        assert!(exited);
        assert!(!attached.unwrap());
        assert!(!seize(pid, pid).unwrap());

        let pid = std::process::id() as i32;
        assert!(!thread_exited(pid, pid));
        assert_eq!(
            seize(pid, pid).unwrap_err().raw_os_error(),
            Some(libc::EPERM)
        );
    }
}
