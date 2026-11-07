//! Stopped-thread capture for initial and periodic sampling. The guard owns the attachment on every error path.
//!
//! # Interface
//! - [`perform_ptrace_request`] (`pub(super)`): `fn perform_ptrace_request( operation: libc::c_uint, tid: i32, address: usize, data: usize, ) -> Result<()>`.
//! - [`capture_thread_snapshot`] (`pub(super)`): `fn capture_thread_snapshot(tid: i32, maps: &[MemoryMap]) -> Result<RawSample>`.
//!
//! Supporting types: [`MemoryMap`], [`RawSample`].
#[cfg(target_arch = "aarch64")]
use super::arch::aarch64::read_registers;
#[cfg(target_arch = "x86_64")]
use super::arch::x86_64::read_registers;
use super::sample::{RawSample, SampleSource, monotonic_ns};
use crate::privileged_helper::process::{maps::MemoryMap, memory};
use anyhow::{Result, ensure};

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
impl PtraceGuard {
    fn wait(&mut self) -> Result<()> {
        loop {
            let mut status = 0;
            let result = unsafe { libc::waitpid(self.tid, &mut status, libc::__WALL) };
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
            self.stopped = true;
            // Preserve signal-delivery stops; synthetic PTRACE_EVENT_STOP carries no signal.
            if status >> 16 == 0 {
                self.signal = libc::WSTOPSIG(status) as usize;
            }
            return Ok(());
        }
    }
}
impl Drop for PtraceGuard {
    fn drop(&mut self) {
        if !self.stopped {
            let _ = perform_ptrace_request(libc::PTRACE_INTERRUPT, self.tid, 0, 0);
            let _ = self.wait();
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
    perform_ptrace_request(libc::PTRACE_SEIZE, tid, 0, 0)?;
    let mut attachment = PtraceGuard {
        tid,
        stopped: false,
        signal: 0,
    };
    perform_ptrace_request(libc::PTRACE_INTERRUPT, tid, 0, 0)?;
    attachment.wait()?;
    let time_ns = monotonic_ns();
    let values = read_registers(tid)?;
    let sp = values.sp();
    let stack = match maps.iter().find(|map| map.contains(sp)) {
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
    use super::*;
    use crate::privileged_helper::process::{self, Target};

    #[test]
    fn guard_detaches_after_capture_error() {
        let target = Target::new("sleeping");
        let failed = (|| -> Result<()> {
            perform_ptrace_request(libc::PTRACE_SEIZE, target.id.pid, 0, 0)?;
            let mut attachment = PtraceGuard {
                tid: target.id.pid,
                stopped: false,
                signal: 0,
            };
            perform_ptrace_request(libc::PTRACE_INTERRUPT, target.id.pid, 0, 0)?;
            attachment.wait()?;
            // Exercise the same early return used for failed GETREGSET/readv calls.
            perform_ptrace_request(
                libc::PTRACE_GETREGSET,
                target.id.pid,
                libc::NT_PRSTATUS as usize,
                0,
            )
        })();
        assert!(failed.is_err());
        target.assert_detached();
    }

    #[test]
    fn already_traced_thread_failure_keeps_perf_sampler_usable() {
        let target = Target::new("sleeping");
        let maps = process::maps::read_maps(target.id.pid).unwrap();
        perform_ptrace_request(libc::PTRACE_SEIZE, target.id.pid, 0, 0).unwrap();
        let attachment = PtraceGuard {
            tid: target.id.pid,
            stopped: false,
            signal: 0,
        };
        let mut sampler = super::super::Sampler::new(target.id);
        sampler.bootstrap(&maps).unwrap();
        let value = serde_json::to_value(sampler.latest()).unwrap();
        assert!(
            value[0]["error"]
                .as_str()
                .unwrap()
                .contains("ptrace sampling")
        );
        drop(attachment);
        sampler.poll(&maps).unwrap();
        target.assert_detached();
    }
}
