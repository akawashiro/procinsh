use super::procfs;
use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
pub(in crate::http_server) struct ProcessId {
    pub(in crate::http_server) pid: i32,
    pub(in crate::http_server) start_time_ticks: u64,
}

#[cfg(test)]
pub(in crate::http_server) fn identity(pid: i32) -> Result<ProcessId> {
    ensure!(pid > 0, "PID must be positive");
    let stat = procfs::read_stat(&format!("/proc/{pid}/stat"))?;
    Ok(ProcessId {
        pid,
        start_time_ticks: stat.start_time,
    })
}

pub(in crate::http_server) fn check_identity(id: ProcessId) -> Result<()> {
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
