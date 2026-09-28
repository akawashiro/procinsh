// Session handles and lifecycle methods are re-exported by the process façade.
use super::SubscribeError;
mod history;
use history::HistoryPoint;

use crate::http_server::process::{
    self, ProcessId, ProcessSummary,
    maps::{self, MemoryMap, MemoryRollup},
    procfs::{self, IoStats},
    threads::{self, ThreadObservation},
};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::watch;

#[derive(Clone, Debug, Default, Serialize)]
struct Rates {
    minor_faults: Option<f64>,
    major_faults: Option<f64>,
    voluntary_context_switches: Option<f64>,
    nonvoluntary_context_switches: Option<f64>,
    read_bytes: Option<f64>,
    write_bytes: Option<f64>,
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct ProcessObservation {
    timestamp: u64,
    process_id: ProcessId,
    cpu_percent: Option<f64>,
    rss_bytes: u64,
    vms_bytes: u64,
    minor_faults: u64,
    major_faults: u64,
    voluntary_context_switches: Option<u64>,
    nonvoluntary_context_switches: Option<u64>,
    io: Option<IoStats>,
    rates: Rates,
    pub(super) threads: Vec<ThreadObservation>,
    cpu: i32,
    nice: i64,
    priority: i64,
    #[serde(skip)]
    ticks: u64,
    #[serde(skip)]
    measured_at: Option<Instant>,
}

pub(super) fn observation(
    id: ProcessId,
    previous: Option<&ProcessObservation>,
) -> Result<ProcessObservation> {
    process::check_identity(id)?;
    let stat = procfs::read_stat(&format!("/proc/{}/stat", id.pid))?;
    let now = Instant::now();
    let mut ts: Vec<_> = threads::tids(id.pid)?
        .into_iter()
        .filter_map(|tid| threads::read(id.pid, tid).ok())
        .collect();
    // /proc/PID/status contains only the leader's context switches: sum all live threads.
    let voluntary = ts
        .iter()
        .map(|t| t.voluntary_context_switches)
        .sum::<Option<u64>>();
    let nonvoluntary = ts
        .iter()
        .map(|t| t.nonvoluntary_context_switches)
        .sum::<Option<u64>>();
    let io = procfs::read_io(id.pid).ok();
    let mut rates = Rates::default();
    let mut cpu_percent = None;
    if let Some(p) = previous.filter(|p| p.process_id == id) {
        let elapsed = now
            .duration_since(p.measured_at.unwrap_or(now))
            .as_secs_f64();
        if elapsed > 0.0 {
            let rate = |a: u64, b: u64| a.checked_sub(b).map(|d| d as f64 / elapsed);
            cpu_percent = rate(stat.ticks, p.ticks).map(|r| r / procfs::ticks_per_second() * 100.0);
            rates.minor_faults = rate(stat.minor_faults, p.minor_faults);
            rates.major_faults = rate(stat.major_faults, p.major_faults);
            // Match thread identities so churn cannot make process context-switch rates negative.
            let mut v = Some(0u64);
            let mut n = Some(0u64);
            for t in &mut ts {
                if let Some(old) = p
                    .threads
                    .iter()
                    .find(|old| old.tid == t.tid && old.start_time == t.start_time)
                {
                    t.cpu_percent =
                        rate(t.ticks, old.ticks).map(|r| r / procfs::ticks_per_second() * 100.0);
                    v = v
                        .zip(
                            t.voluntary_context_switches
                                .zip(old.voluntary_context_switches)
                                .and_then(|(a, b)| a.checked_sub(b)),
                        )
                        .map(|(a, b)| a + b);
                    n = n
                        .zip(
                            t.nonvoluntary_context_switches
                                .zip(old.nonvoluntary_context_switches)
                                .and_then(|(a, b)| a.checked_sub(b)),
                        )
                        .map(|(a, b)| a + b);
                }
            }
            rates.voluntary_context_switches = v.map(|v| v as f64 / elapsed);
            rates.nonvoluntary_context_switches = n.map(|v| v as f64 / elapsed);
            if let (Some(a), Some(b)) = (&io, &p.io) {
                rates.read_bytes = rate(a.read_bytes, b.read_bytes);
                rates.write_bytes = rate(a.write_bytes, b.write_bytes);
            }
        }
    }
    process::check_identity(id)?;
    Ok(ProcessObservation {
        timestamp: process::timestamp_ms(),
        process_id: id,
        cpu_percent,
        rss_bytes: stat.rss,
        vms_bytes: stat.vms,
        minor_faults: stat.minor_faults,
        major_faults: stat.major_faults,
        voluntary_context_switches: voluntary,
        nonvoluntary_context_switches: nonvoluntary,
        io,
        rates,
        threads: ts,
        cpu: stat.cpu,
        nice: stat.nice,
        priority: stat.priority,
        ticks: stat.ticks,
        measured_at: Some(now),
    })
}

#[derive(Clone, Debug, Serialize)]
pub(in crate::http_server) struct Target {
    summary: ProcessSummary,
    pub(in crate::http_server) exited: bool,
    error: Option<String>,
    observation: Option<ProcessObservation>,
    history: VecDeque<HistoryPoint>,
    maps: Vec<MemoryMap>,
    maps_error: Option<String>,
    maps_captured_at: Option<u64>,
    rollup: Option<MemoryRollup>,
}
pub(in crate::http_server) struct Monitoring {
    interval: Duration,
    stopped: AtomicBool,
    viewers: Mutex<usize>,
    workers: Mutex<Vec<std::thread::JoinHandle<()>>>,
}
pub(in crate::http_server) struct ObservationPermit {
    state: Arc<Monitoring>,
    cancelled: Arc<AtomicBool>,
}
impl Drop for ObservationPermit {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        *self.state.viewers.lock().unwrap() -= 1;
    }
}
pub(in crate::http_server) struct ObservationSession {
    pub(in crate::http_server) receiver: watch::Receiver<Target>,
    _permit: ObservationPermit,
}
impl Monitoring {
    pub(in crate::http_server) fn new(interval: Duration) -> Self {
        Self {
            interval,
            stopped: AtomicBool::new(false),
            viewers: Mutex::new(0),
            workers: Mutex::new(Vec::new()),
        }
    }
    #[cfg(test)]
    pub(in crate::http_server) fn observer_count(&self) -> usize {
        *self.viewers.lock().unwrap()
    }
    pub(in crate::http_server) fn reserve(
        self: &Arc<Self>,
    ) -> Result<ObservationPermit, SubscribeError> {
        let mut viewers = self.viewers.lock().unwrap();
        if self.is_stopped() {
            return Err(SubscribeError::Stopped);
        }
        if *viewers >= 32 {
            return Err(SubscribeError::TooManySubscribers);
        }
        *viewers += 1;
        Ok(ObservationPermit {
            state: self.clone(),
            cancelled: Arc::new(AtomicBool::new(false)),
        })
    }
    pub(in crate::http_server) fn observe(
        self: &Arc<Self>,
        id: ProcessId,
        permit: ObservationPermit,
    ) -> Result<ObservationSession> {
        let mut target = capture_target(id)?;
        let (tx, receiver) = watch::channel(target.clone());
        let state = self.clone();
        let cancelled = permit.cancelled.clone();
        let mut workers = self.workers.lock().unwrap();
        ensure!(!self.is_stopped(), "Server is stopping");
        // Reap completed collectors without retaining handles indefinitely.
        let mut pending = Vec::new();
        for worker in workers.drain(..) {
            if worker.is_finished() {
                if worker.join().is_err() {
                    log::error!("process collector thread panicked");
                }
            } else {
                pending.push(worker);
            }
        }
        *workers = pending;
        let worker = std::thread::Builder::new()
            .name(format!("observe-{}", id.pid))
            .spawn(move || {
                log::info!(
                    "process observation started pid={} start_time_ticks={}",
                    id.pid,
                    id.start_time_ticks
                );
                let mut maps_at = Instant::now();
                loop {
                    let start = Instant::now();
                    while start.elapsed() < state.interval
                        && !state.is_stopped()
                        && !cancelled.load(Ordering::Relaxed)
                    {
                        std::thread::sleep(
                            Duration::from_millis(50)
                                .min(state.interval.saturating_sub(start.elapsed())),
                        );
                    }
                    if state.is_stopped() || cancelled.load(Ordering::Relaxed) || tx.is_closed() {
                        break;
                    }
                    match observation(id, target.observation.as_ref()) {
                        Ok(o) => {
                            history::push(&mut target.history, &o);
                            target.observation = Some(o);
                            if target.error.take().is_some() {
                                log::info!("observation recovered pid={}", id.pid);
                            }
                            if maps_at.elapsed() >= Duration::from_secs(5) {
                                refresh_maps(&mut target);
                                maps_at = Instant::now();
                            }
                        }
                        Err(e) => {
                            let error = format!("{e:#}");
                            if target.error.as_ref() != Some(&error) {
                                log::warn!("observation failed pid={}: {error}", id.pid);
                            }
                            target.error = Some(error);
                            target.exited = process::check_identity(id)
                                .err()
                                .is_some_and(|e| e.to_string().starts_with("Process exited"));
                        }
                    }
                    tx.send_replace(target.clone());
                    if target.exited {
                        log::info!("process exited pid={}", id.pid);
                        break;
                    }
                }
                log::info!("process observation stopped pid={}", id.pid);
            })?;
        workers.push(worker);
        Ok(ObservationSession {
            receiver,
            _permit: permit,
        })
    }
    pub(in crate::http_server) fn stop(&self) {
        let _workers = self.workers.lock().unwrap();
        self.stopped.store(true, Ordering::Relaxed);
    }
    pub(in crate::http_server) fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }
    pub(in crate::http_server) fn join_collectors(&self) -> Result<()> {
        let workers = std::mem::take(&mut *self.workers.lock().unwrap());
        let mut failed = false;
        for worker in workers {
            failed |= worker.join().is_err();
        }
        ensure!(!failed, "process collector thread panicked");
        Ok(())
    }
}
pub(super) fn capture_target(id: ProcessId) -> Result<Target> {
    process::check_identity(id)?;
    let stat = procfs::read_stat(&format!("/proc/{}/stat", id.pid))?;
    let summary = process::summary(&stat, &process::users());
    ensure!(summary.identity == id, "Process exited (PID reused)");
    let observation = observation(id, None)?;
    let mut history = VecDeque::new();
    history::push(&mut history, &observation);
    let mut target = Target {
        summary,
        exited: false,
        error: None,
        observation: Some(observation),
        history,
        maps: Vec::new(),
        maps_error: None,
        maps_captured_at: None,
        rollup: None,
    };
    refresh_maps(&mut target);
    process::check_identity(id)?;
    Ok(target)
}
fn refresh_maps(target: &mut Target) {
    let id = target.summary.identity;
    match maps::read(id.pid, true).and_then(|m| {
        process::check_identity(id)?;
        Ok(m)
    }) {
        Ok(maps) => {
            target.maps = maps;
            if target.maps_error.take().is_some() {
                log::info!("memory maps recovered pid={}", id.pid);
            }
            target.maps_captured_at = Some(process::timestamp_ms());
            target.rollup = maps::rollup(id.pid);
        }
        Err(e) => {
            target.maps.clear();
            let error = process::permission_help("memory maps", e);
            if target.maps_error.as_ref() != Some(&error) {
                log::warn!("memory maps failed pid={}: {error}", id.pid);
            }
            target.maps_error = Some(error);
            target.rollup = None;
        }
    }
}

#[cfg(test)]
mod tests;
