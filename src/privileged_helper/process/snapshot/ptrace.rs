//! Stopped-thread capture for initial and periodic sampling. The guard owns the attachment on every error path.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`perform_ptrace_request`] | `pub(super)` | `fn perform_ptrace_request( operation: libc::c_uint, tid: i32, address: usize, data: usize, ) -> `[`Result`]`<()>` |
//! | [`capture_thread_snapshot`] | `pub(super)` | `fn capture_thread_snapshot(tid: i32, maps: &[`[`MemoryMap`]`]) -> `[`Result`]`<`[`RawSample`]`>` |

#[cfg(target_arch = "aarch64")]
use super::arch::aarch64::read_registers;
#[cfg(target_arch = "x86_64")]
use super::arch::x86_64::read_registers;
use crate::{ipc::model::memory::MemoryMap, privileged_helper::process::memory};
use anyhow::{Result, ensure};
use {
    super::sample::{RawSample, monotonic_ns},
    crate::ipc::acquisition::SampleSource,
};

pub(super) fn perform_ptrace_request(
    operation: libc::c_uint,
    tid: i32,
    address: usize,
    data: usize,
) -> Result<()> {
    let result = unsafe {
        libc::ptrace(
            operation,
            tid,
            address as *mut libc::c_void,
            data as *mut libc::c_void,
        )
    };
    if result == -1 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

/// Owns a ptrace attachment and attempts to detach the thread when dropped.
struct PtraceGuard {
    tid: i32,
    stopped: bool,
    signal: usize,
}
fn wait_ptrace_guard(state: &mut PtraceGuard) -> Result<()> {
    loop {
        let mut status = 0;
        let result = unsafe { libc::waitpid(state.tid, &mut status, libc::__WALL) };
        if result == -1 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error.into());
        }
        ensure!(
            libc::WIFSTOPPED(status),
            "Thread exited during ptrace sampling"
        );
        state.stopped = true;
        // Preserve signal-delivery stops; synthetic PTRACE_EVENT_STOP carries no signal.
        if status >> 16 == 0 {
            state.signal = libc::WSTOPSIG(status) as usize;
        }
        return Ok(());
    }
}
impl Drop for PtraceGuard {
    fn drop(&mut self) {
        if !self.stopped {
            let _ = perform_ptrace_request(libc::PTRACE_INTERRUPT, self.tid, 0, 0);
            let _ = wait_ptrace_guard(&mut *self);
        }
        if self.stopped {
            // Retry interrupted syscalls so an error path does not leave a tracee stopped.
            loop {
                match perform_ptrace_request(libc::PTRACE_DETACH, self.tid, 0, self.signal) {
                    Err(error)
                        if error
                            .downcast_ref::<std::io::Error>()
                            .is_some_and(|e| e.kind() == std::io::ErrorKind::Interrupted) =>
                    {
                        continue;
                    }
                    Err(error) => {
                        log::warn!("ptrace detach tid={}: {error}", self.tid);
                        break;
                    }
                    Ok(()) => break,
                }
            }
        }
    }
}

pub(super) fn capture_thread_snapshot(tid: i32, maps: &[MemoryMap]) -> Result<RawSample> {
    use crate::ipc::{model::memory::memory_map_contains, registers::sp_register_set};

    perform_ptrace_request(libc::PTRACE_SEIZE, tid, 0, 0)?;
    let mut attachment = PtraceGuard {
        tid,
        stopped: false,
        signal: 0,
    };
    perform_ptrace_request(libc::PTRACE_INTERRUPT, tid, 0, 0)?;
    wait_ptrace_guard(&mut attachment)?;
    let time_ns = monotonic_ns();
    let values = read_registers(tid)?;
    let sp = sp_register_set(&values);
    let stack = match maps.iter().find(|map| memory_map_contains(map, sp)) {
        _ if memory::privacy_enabled() => Vec::new(),
        Some(map) => memory::read_process_memory(tid, sp, (map.end - sp).min(16 * 1024) as usize)?,
        None => Vec::new(),
    };
    Ok(RawSample {
        tid,
        time_ns,
        cpu: None,
        registers: values,
        stack,
        source: SampleSource::Ptrace,
    })
}

#[cfg(test)]
mod tests {
    use {super::*, crate::privileged_helper::process};

    #[test]
    fn guard_detaches_after_capture_error() {
        use crate::privileged_helper::process::{
            create_target, test_support::assert_detached_target,
        };

        let target = create_target("sleeping");
        let failed = (|| -> Result<()> {
            perform_ptrace_request(libc::PTRACE_SEIZE, target.id.pid, 0, 0)?;
            let mut attachment = PtraceGuard {
                tid: target.id.pid,
                stopped: false,
                signal: 0,
            };
            perform_ptrace_request(libc::PTRACE_INTERRUPT, target.id.pid, 0, 0)?;
            super::wait_ptrace_guard(&mut attachment)?;
            // Exercise the same early return used for failed GETREGSET/readv calls.
            perform_ptrace_request(
                libc::PTRACE_GETREGSET,
                target.id.pid,
                libc::NT_PRSTATUS as usize,
                0,
            )
        })();
        assert!(failed.is_err());
        assert_detached_target(&target);
    }

    #[test]
    fn already_traced_thread_failure_keeps_perf_sampler_usable() {
        use crate::privileged_helper::process::{
            create_target,
            snapshot::{bootstrap_sampler, create_sampler, latest_sampler, poll_sampler},
            test_support::assert_detached_target,
        };

        let target = create_target("sleeping");
        let maps = process::maps::read_maps(target.id).unwrap();
        perform_ptrace_request(libc::PTRACE_SEIZE, target.id.pid, 0, 0).unwrap();
        let attachment = PtraceGuard {
            tid: target.id.pid,
            stopped: false,
            signal: 0,
        };
        let mut sampler = create_sampler(target.id);
        bootstrap_sampler(&mut sampler, &maps).unwrap();
        let value = serde_json::to_value(latest_sampler(&sampler)).unwrap();
        assert!(
            value[0]["error"]
                .as_str()
                .unwrap()
                .contains("ptrace sampling")
        );
        drop(attachment);
        poll_sampler(&mut sampler, &maps).unwrap();
        assert_detached_target(&target);
    }
}
