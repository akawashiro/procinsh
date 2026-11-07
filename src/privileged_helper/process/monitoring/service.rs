//! Manage process observation sessions, collection workers and shutdown.
//!
//! # Interface
//! - [`ProcessMonitor`] (`pub(in crate::privileged_helper)`): `struct ProcessMonitor`.
//! - [`ObservationPermit`] (`pub(in crate::privileged_helper)`): `struct ObservationPermit`.
//! - [`ObservationSession`] (`pub(in crate::privileged_helper)`): `struct ObservationSession`.
//! - [`capture_target`] (`pub(in crate::privileged_helper::process)`):
//!   `fn capture_target(id: `[`ProcessId`]`) -> `[`anyhow::Result`]`<(`[`ProcessDetailSnapshot`]`, `[`ProcessSample`]`)>`.

#[cfg(test)]
use crate::http_server::model::process::{initial_observation, next_observation};
#[cfg(test)]
use std::collections::VecDeque;
// Session handles and lifecycle methods are re-exported by the process façade.
#[cfg(test)]
use crate::http_server::analysis::history;
use crate::privileged_helper::process::SubscribeError;
use {super::sampling::capture_sample, crate::ipc::model::process::ProcessSample};

use crate::{
    ipc::{model::memory::MemoryMap, schema::ProcessId},
    privileged_helper::process::{self, maps, procfs},
};
use anyhow::{Result, ensure};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::watch;

use crate::ipc::acquisition::ProcessDetailSnapshot;
/// Manages process observation sessions, collector workers and shutdown.
pub(in crate::privileged_helper) struct ProcessMonitor {
    interval: Duration,
    stopped: AtomicBool,
    viewers: Mutex<usize>,
    workers: Mutex<Vec<std::thread::JoinHandle<()>>>,
}
pub(in crate::privileged_helper) struct ObservationPermit {
    state: Arc<ProcessMonitor>,
    cancelled: Arc<AtomicBool>,
}
impl Drop for ObservationPermit {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        *self.state.viewers.lock().unwrap() -= 1;
    }
}
pub(in crate::privileged_helper) struct ObservationSession {
    pub(in crate::privileged_helper) receiver: watch::Receiver<ProcessDetailSnapshot>,
    _permit: ObservationPermit,
}
impl ProcessMonitor {
    pub(in crate::privileged_helper) fn new(interval: Duration) -> Self {
        Self {
            interval,
            stopped: AtomicBool::new(false),
            viewers: Mutex::new(0),
            workers: Mutex::new(Vec::new()),
        }
    }

    #[cfg(test)]
    pub(in crate::privileged_helper) fn observer_count(&self) -> usize {
        *self.viewers.lock().unwrap()
    }

    pub(in crate::privileged_helper) fn reserve(
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

    pub(in crate::privileged_helper) fn observe(
        self: &Arc<Self>,
        id: ProcessId,
        permit: ObservationPermit,
    ) -> Result<ObservationSession> {
        let (mut target, mut _previous_sample) = capture_target(id)?;
        // Cache structural mappings for the sampler, updating them at each map refresh.
        let mut structural_maps: Vec<MemoryMap> =
            target.maps.iter().map(|m| m.mapping.clone()).collect();
        let mut sampler = process::snapshot::Sampler::new(id);
        if let Err(error) = sampler.bootstrap(&structural_maps) {
            target.sampling_error = Some(format!("{error:#}"));
        }
        target.live_samples = sampler.latest();
        target.mapping_snapshots = sampler.mapping_snapshots();
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
                        match sampler.poll(&structural_maps) {
                            Ok(()) => target.sampling_error = None,
                            Err(e) => target.sampling_error = Some(e.to_string()),
                        }
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
                            #[cfg(test)]
                            {
                                let o = next_observation(&_previous_sample, &current);
                                _previous_sample = current.clone();
                                history::append_history_observation(&mut target.history, &o);
                                target.observation = Some(o);
                            }
                            target.process_sample = current;
                            if target.error.take().is_some() {
                                log::info!("observation recovered pid={}", id.pid);
                            }
                            if maps_at.elapsed() >= Duration::from_secs(5) {
                                refresh_maps(&mut target);
                                structural_maps =
                                    target.maps.iter().map(|m| m.mapping.clone()).collect();
                                maps_at = Instant::now();
                            }
                        }
                        Err(e) => {
                            let error = format!("{e:#}");
                            if target.error.as_ref() != Some(&error) {
                                log::warn!("observation failed pid={}: {error}", id.pid);
                            }
                            target.error = Some(error);
                            target.exited = process::validate_process_identity(id)
                                .err()
                                .is_some_and(|e| e.to_string().starts_with("Process exited"));
                        }
                    }
                    target.live_samples = sampler.latest();
                    target.mapping_snapshots = sampler.mapping_snapshots();
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

    pub(in crate::privileged_helper) fn stop(&self) {
        let _workers = self.workers.lock().unwrap();
        self.stopped.store(true, Ordering::Relaxed);
    }

    pub(in crate::privileged_helper) fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }

    pub(in crate::privileged_helper) fn join_collectors(&self) -> Result<()> {
        let workers = std::mem::take(&mut *self.workers.lock().unwrap());
        let mut failed = false;
        for worker in workers {
            failed |= worker.join().is_err();
        }
        ensure!(!failed, "process collector thread panicked");
        Ok(())
    }
}

pub(in crate::privileged_helper::process) fn capture_target(
    id: ProcessId,
) -> Result<(ProcessDetailSnapshot, ProcessSample)> {
    process::validate_process_identity(id)?;
    let stat = procfs::read_process_stat(&format!("/proc/{}/stat", id.pid))?;
    let summary = process::collect_process_summary(&stat, &process::read_user_names());
    ensure!(summary.identity == id, "Process exited (PID reused)");
    let sample = capture_sample(id)?;
    #[cfg(test)]
    let observation = initial_observation(&sample);
    #[cfg(test)]
    let mut history = VecDeque::new();
    #[cfg(test)]
    history::append_history_observation(&mut history, &observation);
    let mut target = ProcessDetailSnapshot {
        summary,
        process_sample: sample.clone(),
        exited: false,
        error: None,
        #[cfg(test)]
        observation: Some(observation),
        #[cfg(test)]
        history,
        maps: Vec::new(),
        maps_error: None,
        maps_captured_at: None,
        rollup: None,
        live_samples: Vec::new(),
        mapping_snapshots: Vec::new(),
        sampling_error: None,
    };
    refresh_maps(&mut target);
    process::validate_process_identity(id)?;
    Ok((target, sample))
}

fn refresh_maps(target: &mut ProcessDetailSnapshot) {
    let id = target.summary.identity;
    match maps::read_smaps(id.pid).and_then(|m| {
        process::validate_process_identity(id)?;
        Ok(m)
    }) {
        Ok(maps) => {
            target.maps = maps;
            if target.maps_error.take().is_some() {
                log::info!("memory maps recovered pid={}", id.pid);
            }
            target.maps_captured_at = Some(process::timestamp_ms());
            target.rollup = maps::read_memory_rollup(id.pid);
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
