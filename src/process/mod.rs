pub mod details;
pub mod discovery;
pub mod fds;
pub mod maps;
pub mod memory;
pub mod procfs;
pub mod signals;
pub(crate) mod sockets;
pub mod threads;

use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub struct ProcessId {
    pub pid: i32,
    pub start_time_ticks: u64,
}

pub fn identity(pid: i32) -> Result<ProcessId> {
    ensure!(pid > 0, "PID must be positive");
    let stat = procfs::read_stat(&format!("/proc/{pid}/stat"))?;
    Ok(ProcessId {
        pid,
        start_time_ticks: stat.start_time,
    })
}

pub fn check_identity(id: ProcessId) -> Result<()> {
    ensure!(id.pid > 0, "PID must be positive");
    let stat = match procfs::read_stat(&format!("/proc/{}/stat", id.pid)) {
        Ok(stat) => stat,
        Err(error) => {
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| matches!(e.raw_os_error(), Some(libc::ENOENT) | Some(libc::ESRCH)))
            {
                bail!("Process exited");
            }
            return Err(error);
        }
    };
    ensure!(
        stat.start_time == id.start_time_ticks,
        "Process exited (or PID was reused)"
    );
    ensure!(
        !matches!(stat.state.as_str(), "Z" | "X" | "x"),
        "Process exited"
    );
    Ok(())
}

pub fn timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

pub fn permission_help(operation: &str, error: impl std::fmt::Display) -> String {
    format!(
        "{operation}: {error}. If access is denied, check the process owner, kernel.yama.ptrace_scope, and CAP_SYS_PTRACE. sudo is never run automatically."
    )
}
