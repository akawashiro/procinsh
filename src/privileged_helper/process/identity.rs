//! Validate process identity against procfs to detect exit and PID reuse.
//!
//! # Interface
//! [`validate_process_identity`] (`pub(in crate::privileged_helper)`):
//! `fn validate_process_identity(id: `[`ProcessId`]`) -> `[`anyhow::Result`]`<()>`.

use super::procfs;
use crate::ipc::schema::ProcessId;
use anyhow::{Result, bail, ensure};

#[cfg(test)]
pub(in crate::privileged_helper) fn read_process_identity(pid: i32) -> Result<ProcessId> {
    ensure!(pid > 0, "PID must be positive");
    let stat = procfs::read_process_stat(&format!("/proc/{pid}/stat"))?;
    Ok(ProcessId {
        pid,
        start_time_ticks: stat.start_time,
    })
}

pub(in crate::privileged_helper) fn validate_process_identity(id: ProcessId) -> Result<()> {
    ensure!(id.pid > 0, "PID must be positive");
    let stat = match procfs::read_process_stat(&format!("/proc/{}/stat", id.pid)) {
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
