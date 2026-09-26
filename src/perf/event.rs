use super::{decode::Format, ring::Ring};
use anyhow::{Context, Result};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

// Linux UAPI perf_event_attr, through sig_data (128 bytes). All fields used
// below have fixed UAPI offsets; the u64 array also supplies 8-byte alignment.
#[repr(C)]
struct Attr([u64; 16]);
impl Attr {
    fn new(hz: u32, format: Format) -> Self {
        let mut a = Self([0; 16]);
        a.0[0] = 1 | (128 << 32); // SOFTWARE, size
        a.0[1] = 0; // CPU_CLOCK
        a.0[2] = u64::from(hz);
        a.0[3] = format.sample_type;
        // disabled, exclude_kernel/hv, comm, freq, task, exclude_callchain_kernel,
        // comm_exec, use_clockid, remove_on_exec. Require exec-safe kernel support.
        a.0[5] = 1
            | (1 << 5)
            | (1 << 6)
            | (1 << 9)
            | (1 << 10)
            | (1 << 13)
            | (1 << 21)
            | (1 << 24)
            | (1 << 25)
            | (1 << 36);
        a.0[6] = 1; // wakeup_events
        a.0[10] = format.regs_mask;
        a.0[11] = (libc::CLOCK_MONOTONIC as u64) << 32;
        a
    }
}
pub struct Event {
    pub fd: OwnedFd,
    pub ring: Ring,
    pub format: Format,
}
impl Event {
    pub fn open(tid: i32, hz: u32, callchain: bool) -> Result<(Self, Option<String>)> {
        let formats = if callchain {
            vec![Format::full(), Format::registers(), Format::ip()]
        } else {
            vec![Format::registers(), Format::ip()]
        };
        let mut warnings = Vec::new();
        for format in formats {
            match Self::try_open(tid, hz, format) {
                Ok(event) => {
                    return Ok((event, (!warnings.is_empty()).then(|| warnings.join("; "))));
                }
                Err(e) => {
                    let errno = e
                        .downcast_ref::<std::io::Error>()
                        .and_then(|e| e.raw_os_error());
                    if !matches!(errno, Some(libc::EINVAL | libc::EOPNOTSUPP)) {
                        return Err(e);
                    }
                    warnings.push(format!(
                        "perf format {} unsupported: {e}",
                        format.sample_type
                    ));
                }
            }
        }
        anyhow::bail!(
            "{}; requires MONOTONIC and remove_on_exec support",
            warnings.join("; ")
        )
    }
    fn try_open(tid: i32, hz: u32, format: Format) -> Result<Self> {
        let attr = Attr::new(hz, format);
        // SAFETY: attr is a correctly aligned initialized 128-byte UAPI buffer.
        let fd = unsafe { libc::syscall(libc::SYS_perf_event_open, &attr, tid, -1, -1, 8) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error()).context("perf_event_open");
        }
        // SAFETY: successful syscall returned a new owned descriptor.
        let fd = unsafe { OwnedFd::from_raw_fd(fd as i32) };
        let ring = Ring::new(fd.as_raw_fd())?;
        Ok(Self { fd, ring, format })
    }
    pub fn enable(&self) -> Result<()> {
        // PERF_EVENT_IOC_ENABLE = _IO('$', 0).
        if unsafe { libc::ioctl(self.fd.as_raw_fd(), 0x2400, 0) } < 0 {
            return Err(std::io::Error::last_os_error()).context("enable perf event");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires perf access; intentionally overflows a real ring"]
    fn live_ring_overflow_reports_loss() {
        let tid = unsafe { libc::syscall(libc::SYS_gettid) } as i32;
        let (mut event, _) = Event::open(tid, 199, true).unwrap();
        event.enable().unwrap();
        let busy = |duration| {
            let start = std::time::Instant::now();
            while start.elapsed() < duration {
                std::hint::black_box(1 + 1);
            }
        };
        busy(std::time::Duration::from_secs(2));
        let mut records = event.ring.drain().unwrap();
        busy(std::time::Duration::from_millis(100));
        records.extend(event.ring.drain().unwrap());
        assert!(records.iter().any(|r| matches!(super::super::decode::decode(r,event.format),Ok(super::super::decode::Record::Lost(n)) if n>0)),"overflow must report LOST");
    }
}
