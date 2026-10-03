//! Linux x86-64 perf ABI. Events follow a TID across CPU migration.
use super::{
    registers::RegisterSet,
    sample::{RawSample as Sample, SampleSource},
};
use anyhow::{Context, Result, bail, ensure};
use std::{
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    sync::atomic::{AtomicU64, Ordering},
};

// AX/BX/CX/DX/SI/DI/BP/SP/IP/FLAGS and R8–R15, in kernel register order.
pub(super) const REGS_MASK: u64 = 0x3ff | (0xff << 16);
const SAMPLE_TYPE: u64 = (1 << 1) | (1 << 2) | (1 << 7) | (1 << 12) | (1 << 13);
// perf_event_attr through PERF_ATTR_SIZE_VER3 (96 bytes); newer fields remain zero.
#[repr(C)]
#[derive(Default)]
struct Attr {
    kind: u32,
    size: u32,
    config: u64,
    period: u64,
    sample_type: u64,
    read_format: u64,
    flags: u64,
    wakeup: u32,
    bp_type: u32,
    config1: u64,
    config2: u64,
    branch: u64,
    regs: u64,
    stack: u32,
    clock: i32,
}
pub(super) struct Event {
    _fd: OwnedFd,
    mapping: *mut u8,
    length: usize,
    offset: usize,
    capacity: usize,
    tail: u64,
    pub(super) lost: u64,
    context_switch: bool,
    pending: Option<Sample>,
}
// SAFETY: Event exclusively owns its mmap and fd. Access requires &mut self;
// moving ownership to the observation worker does not share the mapping.
unsafe impl Send for Event {}

impl Event {
    pub(super) fn open(tid: i32) -> Result<Self> {
        let attr = Attr {
            kind: 1,
            size: std::mem::size_of::<Attr>() as u32,
            config: 0,
            period: 10_000_000,
            sample_type: SAMPLE_TYPE,
            // CPU clock, 100 Hz of scheduled user CPU time, CLOCK_MONOTONIC.
            flags: (1 << 5) | (1 << 6) | (1 << 25),
            wakeup: 1,
            regs: REGS_MASK,
            stack: 16384,
            clock: libc::CLOCK_MONOTONIC,
            ..Attr::default()
        };
        Self::open_attr(tid, attr, false)
    }
    pub(super) fn context_switch(tid: i32) -> Result<Self> {
        Self::open_attr(
            tid,
            Attr {
                kind: 1,
                size: std::mem::size_of::<Attr>() as u32,
                config: 3, // PERF_COUNT_SW_CONTEXT_SWITCHES, including kernel switch-out.
                period: 1,
                sample_type: SAMPLE_TYPE,
                flags: (1 << 18) | (1 << 25) | (1 << 26),
                wakeup: 1,
                regs: REGS_MASK,
                stack: 16384,
                clock: libc::CLOCK_MONOTONIC,
                ..Attr::default()
            },
            true,
        )
    }
    fn open_attr(tid: i32, attr: Attr, context_switch: bool) -> Result<Self> {
        let fd = unsafe { libc::syscall(libc::SYS_perf_event_open, &attr, tid, -1, -1, 8) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error())
                .context("perf_event_open (check CAP_PERFMON / perf_event_paranoid)");
        }
        let fd = unsafe { OwnedFd::from_raw_fd(fd as i32) };
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
        let length = page * 17;
        let mapping = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                length,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd.as_raw_fd(),
                0,
            )
        };
        if mapping == libc::MAP_FAILED {
            bail!("perf mmap: {}", std::io::Error::last_os_error());
        }
        let mapping = mapping.cast::<u8>();
        // perf_event_mmap_page ABI: head/tail at 1024/1032, offset/size at 1040/1048.
        let offset = unsafe { std::ptr::read_volatile(mapping.add(1040).cast::<u64>()) } as usize;
        let capacity = unsafe { std::ptr::read_volatile(mapping.add(1048).cast::<u64>()) } as usize;
        if offset < page
            || capacity == 0
            || offset.checked_add(capacity).is_none_or(|end| end > length)
        {
            unsafe {
                libc::munmap(mapping.cast(), length);
            }
            bail!("invalid perf ring layout");
        }
        Ok(Self {
            _fd: fd,
            mapping,
            length,
            offset,
            capacity,
            tail: 0,
            lost: 0,
            context_switch,
            pending: None,
        })
    }
    fn copy(&self, position: u64, length: usize) -> Vec<u8> {
        (0..length)
            .map(|i| unsafe {
                std::ptr::read_volatile(self.mapping.add(
                    self.offset + (position.wrapping_add(i as u64) % self.capacity as u64) as usize,
                ))
            })
            .collect()
    }
    pub(super) fn drain(&mut self) -> Result<Option<Sample>> {
        let head = unsafe { &*self.mapping.add(1024).cast::<AtomicU64>() }.load(Ordering::Acquire);
        let mut latest = None;
        let result = (|| {
            ensure!(
                head.wrapping_sub(self.tail) <= self.capacity as u64,
                "perf ring overrun"
            );
            while self.tail != head {
                ensure!(head.wrapping_sub(self.tail) >= 8, "truncated perf header");
                let header = self.copy(self.tail, 8);
                let kind = u32::from_ne_bytes(header[..4].try_into()?);
                let size = u16::from_ne_bytes(header[6..8].try_into()?) as usize;
                ensure!(
                    size >= 8
                        && size <= self.capacity
                        && size as u64 <= head.wrapping_sub(self.tail),
                    "invalid perf record size"
                );
                let bytes = self.copy(self.tail.wrapping_add(8), size - 8);
                self.tail = self.tail.wrapping_add(size as u64);
                let misc = u16::from_ne_bytes(header[4..6].try_into()?);
                match kind {
                    9 => {
                        let sample = parse(&bytes)?;
                        if self.context_switch {
                            self.pending = sample;
                        } else if sample.is_some() {
                            latest = sample;
                        }
                    }
                    14 if self.context_switch => {
                        // sample_id_all trailer: TID, TIME, CPU. A switch-in,
                        // missing sample, or loss must never classify an old sample.
                        let mut c = Cursor(&bytes);
                        let tids = c.take(8)?;
                        let tid = i32::from_ne_bytes(tids[4..].try_into()?);
                        let time = c.u64()?;
                        let cpu = c.u64()? as u32;
                        if let Some(mut sample) = self.pending.take()
                            && misc & (1 << 13) != 0
                            && sample.tid == tid
                            && sample.time_ns <= time
                            && sample.cpu == Some(cpu)
                        {
                            sample.source = SampleSource::ContextSwitch {
                                preempted: misc & (1 << 14) != 0,
                            };
                            latest = Some(sample);
                        }
                    }
                    5 | 6 if self.context_switch => {
                        self.pending = None;
                    }
                    2 => {
                        self.pending = None;
                        let mut cursor = Cursor(&bytes);
                        cursor.u64()?;
                        self.lost = self.lost.saturating_add(cursor.u64()?);
                    }
                    _ => {}
                }
            }
            Ok(latest)
        })();
        if result.is_err() {
            self.lost = self.lost.saturating_add(1);
            self.tail = head;
            self.pending = None;
        }
        unsafe { &*self.mapping.add(1032).cast::<AtomicU64>() }.store(self.tail, Ordering::Release);
        result
    }
}
impl Drop for Event {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.mapping.cast(), self.length);
        }
    }
}
struct Cursor<'a>(&'a [u8]);
impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        ensure!(n <= self.0.len(), "truncated perf sample");
        let (value, rest) = self.0.split_at(n);
        self.0 = rest;
        Ok(value)
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_ne_bytes(self.take(8)?.try_into()?))
    }
}
fn parse(bytes: &[u8]) -> Result<Option<Sample>> {
    let mut c = Cursor(bytes);
    let tids = c.take(8)?;
    let tid = i32::from_ne_bytes(tids[4..].try_into()?);
    let time_ns = c.u64()?;
    let cpu = c.u64()? as u32;
    let abi = c.u64()?;
    if abi == 0 {
        return Ok(None);
    }
    ensure!(abi == 2, "perf sample is not x86-64 user mode");
    let mut values = [0; 24];
    for (i, value) in values.iter_mut().enumerate() {
        if REGS_MASK & (1 << i) != 0 {
            *value = c.u64()?;
        }
    }
    let length = usize::try_from(c.u64()?)?;
    let stack = c.take(length)?;
    let used = if length == 0 {
        0
    } else {
        usize::try_from(c.u64()?)?
    };
    ensure!(used <= length, "invalid perf dynamic stack size");
    Ok(Some(Sample {
        source: SampleSource::CpuClock,
        tid,
        time_ns,
        cpu: Some(cpu),
        registers: RegisterSet(values),
        stack: stack[..used].to_vec(),
    }))
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fake_event() -> Event {
        let length = 8192;
        let mapping = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                length,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        }
        .cast::<u8>();
        assert_ne!(mapping.cast(), libc::MAP_FAILED);
        Event {
            _fd: std::fs::File::open("/dev/null").unwrap().into(),
            mapping,
            length,
            offset: 4096,
            capacity: 4096,
            tail: 0,
            lost: 0,
            context_switch: true,
            pending: None,
        }
    }
    fn push(event: &Event, kind: u32, misc: u16, payload: &[u8]) {
        let head = unsafe { &*event.mapping.add(1024).cast::<AtomicU64>() };
        let position = head.load(Ordering::Acquire);
        let mut bytes = Vec::new();
        bytes.extend(kind.to_ne_bytes());
        bytes.extend(misc.to_ne_bytes());
        bytes.extend(((payload.len() + 8) as u16).to_ne_bytes());
        bytes.extend(payload);
        for (i, byte) in bytes.iter().enumerate() {
            unsafe {
                *event
                    .mapping
                    .add(event.offset + (position as usize + i) % event.capacity) = *byte;
            }
        }
        head.store(position + bytes.len() as u64, Ordering::Release);
    }
    fn sample_bytes(time: u64) -> Vec<u8> {
        let mut bytes = Vec::new();
        for value in [(42u64 << 32) | 42, time, 7, 2] {
            bytes.extend(value.to_ne_bytes());
        }
        for index in 0..24 {
            if REGS_MASK & (1 << index) != 0 {
                bytes.extend((index as u64 + 100).to_ne_bytes());
            }
        }
        bytes.extend(0u64.to_ne_bytes());
        bytes
    }
    fn switch_bytes(tid: u32, time: u64) -> Vec<u8> {
        [(u64::from(tid) << 32) | 42, time, 7]
            .into_iter()
            .flat_map(u64::to_ne_bytes)
            .collect()
    }
    #[test]
    fn switch_classification_survives_split_drain_and_rejects_loss_and_mismatch() {
        let mut event = fake_event();
        for preempted in [false, true] {
            push(&event, 9, 0, &sample_bytes(100));
            assert!(event.drain().unwrap().is_none());
            push(
                &event,
                14,
                (1 << 13) | if preempted { 1 << 14 } else { 0 },
                &switch_bytes(42, 101),
            );
            let sample = event.drain().unwrap().unwrap();
            assert_eq!(sample.source, SampleSource::ContextSwitch { preempted });
            assert_eq!(sample.registers.0[8], 108);
        }
        for (kind, misc, payload) in [
            (14, 0, switch_bytes(42, 101)),       // switch-in
            (14, 1 << 13, switch_bytes(43, 101)), // wrong TID
            (14, 1 << 13, switch_bytes(42, 99)),  // earlier switch
            (
                2,
                0,
                [1u64, 2].into_iter().flat_map(u64::to_ne_bytes).collect(),
            ),
            (5, 0, Vec::new()), // throttle
            (
                14,
                1 << 13,
                [(42u64 << 32) | 42, 101, 8]
                    .into_iter()
                    .flat_map(u64::to_ne_bytes)
                    .collect(),
            ), // wrong CPU
        ] {
            push(&event, 9, 0, &sample_bytes(100));
            push(&event, kind, misc, &payload);
            push(&event, 14, 1 << 13, &switch_bytes(42, 102));
            assert!(event.drain().unwrap().is_none());
        }
        push(&event, 9, 0, &sample_bytes(100));
        push(&event, 14, 1 << 13, &[0; 8]);
        assert!(event.drain().is_err());
        push(&event, 14, 1 << 13, &switch_bytes(42, 101));
        assert!(event.drain().unwrap().is_none());
    }

    #[test]
    fn ring_wrap_lost_and_overrun_are_consumed() {
        let length = 8192;
        let mapping = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                length,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        }
        .cast::<u8>();
        assert_ne!(mapping.cast(), libc::MAP_FAILED);
        let fd: OwnedFd = std::fs::File::open("/dev/null").unwrap().into();
        let mut event = Event {
            _fd: fd,
            mapping,
            length,
            offset: 4096,
            capacity: 4096,
            tail: 4090,
            lost: 0,
            context_switch: false,
            pending: None,
        };
        let mut bytes = Vec::new();
        bytes.extend(2u32.to_ne_bytes());
        bytes.extend(0u16.to_ne_bytes());
        bytes.extend(24u16.to_ne_bytes());
        bytes.extend(1u64.to_ne_bytes());
        bytes.extend(17u64.to_ne_bytes());
        for (i, b) in bytes.iter().enumerate() {
            unsafe {
                *mapping.add(4096 + (4090 + i) % 4096) = *b;
            }
        }
        unsafe { &*mapping.add(1024).cast::<AtomicU64>() }.store(4114, Ordering::Release);
        assert!(event.drain().unwrap().is_none());
        assert_eq!(event.lost, 17);
        assert_eq!(
            unsafe { &*mapping.add(1032).cast::<AtomicU64>() }.load(Ordering::Acquire),
            4114
        );
        unsafe { &*mapping.add(1024).cast::<AtomicU64>() }.store(10000, Ordering::Release);
        assert!(event.drain().is_err());
        assert_eq!(event.lost, 18);
        assert_eq!(event.tail, 10000);
        assert!(event.drain().unwrap().is_none());
    }
    #[test]
    fn parses_register_order_and_dynamic_stack_and_rejects_truncation() {
        let mut b = Vec::new();
        for x in [(42u64 << 32) | 42, 123, 7, 2] {
            b.extend(x.to_ne_bytes());
        }
        for i in 0..24 {
            if REGS_MASK & (1 << i) != 0 {
                b.extend((i as u64 + 100).to_ne_bytes());
            }
        }
        b.extend(16u64.to_ne_bytes());
        b.extend([1; 16]);
        b.extend(8u64.to_ne_bytes());
        let s = parse(&b).unwrap().unwrap();
        assert_eq!((s.tid, s.cpu, s.time_ns), (42, Some(7), 123));
        assert_eq!(s.registers.0[8], 108);
        assert_eq!(s.stack.len(), 8);
        for n in 0..b.len() {
            assert!(parse(&b[..n]).is_err());
        }
    }
}
