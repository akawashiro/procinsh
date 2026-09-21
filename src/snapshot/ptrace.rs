use crate::process::{self, ProcessId, threads};
use anyhow::{Result, bail, ensure};
use std::time::{Duration, Instant};

struct Tracee {
    tid: i32,
    stopped: bool,
    signal: i32,
    gone: bool,
}
pub struct SnapshotGuard {
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
    pub fn capture(id: ProcessId) -> Result<Self> {
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
                match ptrace(libc::PTRACE_SEIZE, tid, 0) {
                    Ok(()) => guard.tracees.push(Tracee {
                        tid,
                        stopped: false,
                        signal: 0,
                        gone: false,
                    }),
                    Err(e) if gone(&e) => continue,
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
            }) {
                return Ok(guard);
            }
            ensure!(Instant::now() < deadline, "thread set did not stabilize");
        }
        bail!("thread set did not stabilize after 16 passes")
    }
    pub fn tids(&self) -> Vec<i32> {
        self.tracees
            .iter()
            .filter(|t| t.stopped && !t.gone)
            .map(|t| t.tid)
            .collect()
    }
    pub fn registers(&self, tid: i32) -> Result<libc::user_regs_struct> {
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
