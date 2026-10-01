// Session handles and lifecycle methods are re-exported by the process façade.
use super::history::{self, HistoryPoint};
use super::sampling::{
    ProcessObservation, ProcessSample, capture_sample, initial_observation, next_observation,
};
use crate::http_server::process::SubscribeError;

use crate::http_server::process::{
    self, ProcessId, ProcessSummary,
    maps::{self, MemoryMap, MemoryRollup},
    procfs,
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
        let (mut target, mut previous_sample) = capture_target(id)?;
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
                    match capture_sample(id) {
                        Ok(current) => {
                            let o = next_observation(&previous_sample, &current);
                            previous_sample = current;
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
pub(in crate::http_server::process) fn capture_target(
    id: ProcessId,
) -> Result<(Target, ProcessSample)> {
    process::check_identity(id)?;
    let stat = procfs::read_stat(&format!("/proc/{}/stat", id.pid))?;
    let summary = process::summary(&stat, &process::users());
    ensure!(summary.identity == id, "Process exited (PID reused)");
    let sample = capture_sample(id)?;
    let observation = initial_observation(&sample);
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
    Ok((target, sample))
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
#[path = "tests.rs"]
mod tests;
