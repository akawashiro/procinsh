pub mod history;

use crate::process::{
    self, ProcessId,
    discovery::{Discovery, ProcessSummary},
    maps::{self, MemoryMap, MemoryRollup},
    procfs::{self, IoStats},
    threads::{self, ThreadObservation},
};
use anyhow::{Result, ensure};
use serde::Serialize;
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::watch;

#[derive(Clone, Debug, Default, Serialize)]
pub struct Rates {
    pub minor_faults: Option<f64>,
    pub major_faults: Option<f64>,
    pub voluntary_context_switches: Option<f64>,
    pub nonvoluntary_context_switches: Option<f64>,
    pub read_bytes: Option<f64>,
    pub write_bytes: Option<f64>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ProcessObservation {
    pub timestamp: u64,
    pub process_id: ProcessId,
    pub cpu_percent: Option<f64>,
    pub rss_bytes: u64,
    pub vms_bytes: u64,
    pub minor_faults: u64,
    pub major_faults: u64,
    pub voluntary_context_switches: Option<u64>,
    pub nonvoluntary_context_switches: Option<u64>,
    pub io: Option<IoStats>,
    pub rates: Rates,
    pub threads: Vec<ThreadObservation>,
    pub cpu: i32,
    pub nice: i64,
    pub priority: i64,
    #[serde(skip)]
    ticks: u64,
    #[serde(skip)]
    measured_at: Option<Instant>,
}

fn observation(id: ProcessId, previous: Option<&ProcessObservation>) -> Result<ProcessObservation> {
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
pub struct Target {
    pub summary: ProcessSummary,
    pub exited: bool,
    pub error: Option<String>,
    pub observation: Option<ProcessObservation>,
    pub history: VecDeque<history::HistoryPoint>,
    pub maps: Vec<MemoryMap>,
    pub maps_error: Option<String>,
    pub maps_captured_at: Option<u64>,
    pub rollup: Option<MemoryRollup>,
}
pub struct Inner {
    pub target: Option<Target>,
    pub discovery: Discovery,
    maps_at: Option<Instant>,
}
pub struct AppState {
    pub space: Arc<crate::space::Space>,
    pub inner: Mutex<Inner>,
    pub interval: Duration,
    pub events: watch::Sender<String>,
    stopped: AtomicBool,
    pub symbols: Arc<Mutex<crate::symbol::Symbolizer>>,
}
impl AppState {
    pub fn new(interval: Duration) -> Self {
        let (events, _) = watch::channel("null".to_owned());
        Self {
            space: Arc::new(crate::space::Space::default()),
            inner: Mutex::new(Inner {
                target: None,
                discovery: Discovery::default(),
                maps_at: None,
            }),
            interval,
            events,
            stopped: AtomicBool::new(false),
            symbols: Arc::new(Mutex::new(crate::symbol::Symbolizer::default())),
        }
    }
    pub fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn select(&self, id: ProcessId) -> Result<Target> {
        let mut inner = self.lock();
        process::check_identity(id)?;
        let stat = procfs::read_stat(&format!("/proc/{}/stat", id.pid))?;
        let summary = process::discovery::summary(&stat, &process::discovery::users());
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
        inner.target = Some(target.clone());
        inner.maps_at = Some(Instant::now());
        self.publish(&inner);
        Ok(target)
    }
    pub fn publish(&self, inner: &Inner) {
        if let Ok(json) = serde_json::to_string(&inner.target) {
            self.events.send_replace(json);
        }
    }
    pub fn stop(&self) {
        self.space.stop();
        self.stopped.store(true, Ordering::Relaxed);
    }
    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }
    pub fn start_collector(self: &Arc<Self>) -> std::thread::JoinHandle<()> {
        let state = self.clone();
        std::thread::spawn(move || {
            while !state.stopped.load(Ordering::Relaxed) {
                let start = Instant::now();
                {
                    let mut inner = state.lock();
                    let refresh = inner
                        .maps_at
                        .is_none_or(|t| t.elapsed() >= Duration::from_secs(5));
                    if let Some(target) = inner.target.as_mut().filter(|t| !t.exited) {
                        match observation(target.summary.identity, target.observation.as_ref()) {
                            Ok(o) => {
                                history::push(&mut target.history, &o);
                                target.observation = Some(o);
                                target.error = None;
                                if refresh {
                                    refresh_maps(target);
                                }
                            }
                            Err(e) => {
                                target.error = Some(format!("{e:#}"));
                                target.exited = process::check_identity(target.summary.identity)
                                    .err()
                                    .is_some_and(|e| e.to_string().starts_with("Process exited"));
                            }
                        }
                        if refresh {
                            inner.maps_at = Some(Instant::now());
                        }
                        state.publish(&inner);
                    }
                }
                // Short sleeps make shutdown responsive even with a 60-second interval.
                while start.elapsed() < state.interval && !state.stopped.load(Ordering::Relaxed) {
                    std::thread::sleep(
                        Duration::from_millis(50)
                            .min(state.interval.saturating_sub(start.elapsed())),
                    );
                }
            }
        })
    }
}
fn refresh_maps(target: &mut Target) {
    let id = target.summary.identity;
    match maps::read(id.pid, true).and_then(|m| {
        process::check_identity(id)?;
        Ok(m)
    }) {
        Ok(maps) => {
            target.maps = maps;
            target.maps_error = None;
            target.maps_captured_at = Some(process::timestamp_ms());
            target.rollup = maps::rollup(id.pid);
        }
        Err(e) => {
            target.maps.clear();
            target.maps_error = Some(process::permission_help("memory maps", e));
            target.rollup = None;
        }
    }
}
