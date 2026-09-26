use super::{
    Config, Shared,
    decode::{self, Record, Sample},
    event::Event,
    store::ThreadSample,
};
use crate::{
    inspect::{frames::StackFrame, registers},
    process::{self, ProcessId, maps, threads},
    symbol::Symbolizer,
};
use anyhow::{Context, Result};
use std::{
    collections::BTreeMap,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    sync::{Arc, Mutex, atomic::Ordering},
    time::{Duration, Instant},
};

struct ThreadEvent {
    start: u64,
    event: Event,
    throttled: bool,
}
pub(super) fn run(
    id: ProcessId,
    config: Config,
    shared: &Shared,
    symbols: &Arc<Mutex<Symbolizer>>,
) -> Result<()> {
    let ep = unsafe { libc::epoll_create1(libc::EPOLL_CLOEXEC) };
    if ep < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    let ep = unsafe { OwnedFd::from_raw_fd(ep) };
    let mut events: BTreeMap<i32, ThreadEvent> = BTreeMap::new();
    let mut next_scan = Instant::now();
    let mut next_publish = Instant::now();
    let mut pending: BTreeMap<i32, Sample> = BTreeMap::new();
    let mut memory_maps = Vec::new();
    let mut ready = [libc::epoll_event { events: 0, u64: 0 }; 128];
    while !shared.stop.load(Ordering::Acquire) {
        if Instant::now() >= next_scan {
            if process::check_identity(id).is_err() {
                break;
            }
            let mut identities = BTreeMap::new();
            for tid in threads::tids(id.pid)? {
                if let Ok(t) = threads::read(id.pid, tid) {
                    identities.insert(tid, t.start_time);
                }
            }
            // epoll removes closed descriptors automatically.
            events.retain(|tid, e| identities.get(tid) == Some(&e.start));
            pending.retain(|tid, _| events.contains_key(tid));
            let limited = identities.len() > 128;
            let mut warnings = Vec::new();
            for (&tid, &start) in &identities {
                if events.len() >= 128 {
                    break;
                }
                if events.contains_key(&tid) {
                    continue;
                }
                let opened = (|| -> Result<(Event, Option<String>)> {
                    let (event, warning) = Event::open(tid, config.hz, config.callchain)?;
                    // Reject TID reuse during open, not just between scans.
                    anyhow::ensure!(
                        threads::read(id.pid, tid)?.start_time == start,
                        "TID reused during perf open"
                    );
                    process::check_identity(id)?;
                    let mut ee = libc::epoll_event {
                        events: libc::EPOLLIN as u32,
                        u64: tid as u64,
                    };
                    if unsafe {
                        libc::epoll_ctl(
                            ep.as_raw_fd(),
                            libc::EPOLL_CTL_ADD,
                            event.fd.as_raw_fd(),
                            &mut ee,
                        )
                    } < 0
                    {
                        return Err(std::io::Error::last_os_error().into());
                    }
                    event.enable()?;
                    Ok((event, warning))
                })();
                match opened {
                    Ok((event, warning)) => {
                        if let Some(w) = warning {
                            warnings.push(format!("TID {tid}: {w}"));
                        }
                        events.insert(
                            tid,
                            ThreadEvent {
                                start,
                                event,
                                throttled: false,
                            },
                        );
                    }
                    Err(e) => {
                        warnings.push(format!("TID {tid}: {e:#}. Check perf_event_paranoid, CAP_PERFMON, seccomp, RLIMIT_NOFILE and perf_event_mlock_kb."));
                        // Stop probing every TID when a process-wide restriction is likely.
                        if events.is_empty() {
                            break;
                        }
                    }
                }
            }
            for (&tid, e) in &events {
                if e.event.format.sample_type
                    != if config.callchain {
                        decode::Format::full().sample_type
                    } else {
                        decode::Format::registers().sample_type
                    }
                {
                    warnings.push(format!("TID {tid}: reduced perf fields"));
                }
            }
            if limited {
                warnings.push("128-thread sampling limit reached".into());
            }
            memory_maps = maps::read(id.pid, false).unwrap_or_default();
            let mut store = shared.store.lock().unwrap();
            store.retain(&identities);
            store.data.thread_limit_reached = limited;
            store.data.monitored_threads = events.len();
            let status = if events.is_empty() {
                "unavailable"
            } else if events.len() < identities.len() || !warnings.is_empty() {
                "partial"
            } else {
                "active"
            }
            .to_string();
            if store.data.status != status || store.data.warnings != warnings {
                if warnings.is_empty() {
                    log::info!(
                        "perf status pid={} status={} monitored_threads={}",
                        id.pid,
                        status,
                        events.len()
                    );
                } else {
                    log::warn!(
                        "perf status pid={} status={} warnings={}",
                        id.pid,
                        status,
                        warnings.join("; ")
                    );
                }
            }
            store.data.status = status;
            store.data.warnings = warnings;
            next_scan = Instant::now() + Duration::from_secs(1);
        }
        let count =
            unsafe { libc::epoll_wait(ep.as_raw_fd(), ready.as_mut_ptr(), ready.len() as i32, 50) };
        if count < 0 {
            let e = std::io::Error::last_os_error();
            if e.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return Err(e).context("perf epoll_wait");
        }
        let mut ended = Vec::new();
        let mut exec = false;
        for r in &ready[..count as usize] {
            let tid = r.u64 as i32;
            let Some(e) = events.get_mut(&tid) else {
                continue;
            };
            match e.event.ring.drain() {
                Ok(records) => {
                    for bytes in records {
                        match decode::decode(&bytes, e.event.format) {
                            Ok(Record::Sample(sample)) if sample.tid == tid => {
                                // Keep every lightweight IP point, but symbolize only latest samples
                                // once per second, outside the ring draining path.
                                shared.store.lock().unwrap().record(&sample, e.start);
                                pending.insert(tid, sample);
                            }
                            Ok(Record::Lost(n)) => {
                                let mut s = shared.store.lock().unwrap();
                                s.data.lost_total = s.data.lost_total.saturating_add(n);
                            }
                            Ok(Record::Throttle(value)) => e.throttled = value,
                            Ok(Record::Exec) => exec = true,
                            Ok(Record::Exit(exited)) if exited == tid => ended.push(tid),
                            Err(_) => {
                                shared.store.lock().unwrap().data.malformed_total += 1;
                            }
                            _ => {}
                        }
                    }
                }
                Err(error) => {
                    let mut s = shared.store.lock().unwrap();
                    s.data.malformed_total += 1;
                    if s.data.warnings.len() < 128 {
                        s.data.warnings.push(error.to_string());
                    }
                }
            }
            if r.events & libc::EPOLLHUP as u32 != 0 && !ended.contains(&tid) {
                if threads::read(id.pid, tid).is_ok_and(|t| {
                    t.start_time == e.start && !matches!(t.state.as_str(), "Z" | "X" | "x")
                }) {
                    exec = true;
                } else {
                    ended.push(tid);
                }
            }
        }
        if exec {
            let mut s = shared.store.lock().unwrap();
            s.data.warnings.push(
                "Target exec or event hangup: sampling stopped; reopen the detail view to restart"
                    .into(),
            );
            break;
        }
        for tid in ended {
            events.remove(&tid);
            pending.remove(&tid);
        }
        if Instant::now() >= next_publish {
            let mut symbols = symbols.lock().unwrap_or_else(|e| e.into_inner());
            for (tid, raw) in std::mem::take(&mut pending) {
                let Some(e) = events.get(&tid) else {
                    continue;
                };
                let mut sample = make_sample(&raw, e.start, &memory_maps, config.callchain);
                for (index, frame) in sample.call_stack.iter_mut().enumerate() {
                    symbols.resolve(id.pid, &memory_maps, frame, index > 0);
                }
                shared.store.lock().unwrap().push(sample);
            }
            shared.store.lock().unwrap().data.throttled = events.values().any(|e| e.throttled);
            next_publish = Instant::now() + Duration::from_secs(1);
        }
    }
    shared.store.lock().unwrap().data.status = "stopped".into();
    Ok(())
}
fn make_sample(
    raw: &Sample,
    start: u64,
    maps: &[maps::MemoryMap],
    callchain: bool,
) -> ThreadSample {
    ThreadSample {
        tid: raw.tid,
        start_time_ticks: start,
        sampled_at_mono_ns: raw.time.to_string(),
        sample_age_ms: 0,
        ip: raw.ip,
        cpu: raw.cpu,
        registers: registers::from_perf(&raw.regs, maps),
        call_stack: raw.frames.iter().copied().map(StackFrame::raw).collect(),
        unwind_stop: if !callchain {
            "CALLCHAIN disabled"
        } else if raw.frames.is_empty() {
            "No user callchain available"
        } else {
            "User CALLCHAIN; may be partial (frame pointers/depth limit)"
        }
        .into(),
        error: if raw.regs.is_empty() {
            Some(format!("User registers unavailable (ABI {:?})", raw.abi))
        } else {
            None
        },
        quality: if !raw.frames.is_empty() {
            "callchain"
        } else if !raw.regs.is_empty() {
            "registers"
        } else {
            "ip_only"
        }
        .into(),
        time: raw.time,
    }
}
