//! Manage process observation sessions, collection workers and shutdown.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`SubscribeError`] | `pub(in crate::privileged_helper)` | `enum SubscribeError` |
//! | [`ProcessMonitor`] | `pub(in crate::privileged_helper)` | `struct ProcessMonitor` |
//! | [`ObservationPermit`] | `pub(in crate::privileged_helper)` | `struct ObservationPermit` |
//! | [`ObservationSession`] | `pub(in crate::privileged_helper)` | `struct ObservationSession` |
//! | [`create_process_monitor`] | `pub(in crate::privileged_helper)` | `fn create_process_monitor(interval: `[`Duration`]`) -> `[`ProcessMonitor`] |
//! | [`reserve_process_monitor`] | `pub(in crate::privileged_helper)` | `fn reserve_process_monitor( monitor: &`[`Arc`]`<`[`ProcessMonitor`]`>, ) -> `[`Result`]`<`[`ObservationPermit`]`, `[`SubscribeError`]`>` |
//! | [`observe_process_monitor`] | `pub(in crate::privileged_helper)` | `fn observe_process_monitor( monitor: &`[`Arc`]`<`[`ProcessMonitor`]`>, id: `[`ProcessId`]`, permit: `[`ObservationPermit`]`, ) -> `[`Result`]`<`[`ObservationSession`]`>` |
//! | [`stop_process_monitor`] | `pub(in crate::privileged_helper)` | `fn stop_process_monitor(state: &`[`ProcessMonitor`]`)` |
//! | [`process_monitor_is_stopped`] | `pub(in crate::privileged_helper)` | `fn process_monitor_is_stopped(state: &`[`ProcessMonitor`]`) -> bool` |
//! | [`join_process_collectors`] | `pub(in crate::privileged_helper)` | `fn join_process_collectors(state: &`[`ProcessMonitor`]`) -> `[`Result`]`<()>` |

#[cfg(test)]
use crate::http_server::model::process::{initial_observation, next_observation};
#[cfg(test)]
use std::collections::VecDeque;
// Session handles and lifecycle methods are re-exported by the process façade.
#[cfg(test)]
use crate::http_server::analysis::history;
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

/// Failure to reserve a process observation subscription.
#[derive(Debug, PartialEq, Eq)]
pub(in crate::privileged_helper) enum SubscribeError {
    Stopped,
    TooManySubscribers,
}

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
pub(in crate::privileged_helper) fn create_process_monitor(interval: Duration) -> ProcessMonitor {
    ProcessMonitor {
        interval,
        stopped: AtomicBool::new(false),
        viewers: Mutex::new(0),
        workers: Mutex::new(Vec::new()),
    }
}

#[cfg(test)]
pub(in crate::privileged_helper) fn observer_count_process_monitor(
    state: &ProcessMonitor,
) -> usize {
    *state.viewers.lock().unwrap()
}

pub(in crate::privileged_helper) fn reserve_process_monitor(
    monitor: &Arc<ProcessMonitor>,
) -> Result<ObservationPermit, SubscribeError> {
    let mut viewers = monitor.viewers.lock().unwrap();
    if process_monitor_is_stopped(monitor) {
        return Err(SubscribeError::Stopped);
    }
    if *viewers >= 32 {
        return Err(SubscribeError::TooManySubscribers);
    }
    *viewers += 1;
    Ok(ObservationPermit {
        state: monitor.clone(),
        cancelled: Arc::new(AtomicBool::new(false)),
    })
}

pub(in crate::privileged_helper) fn observe_process_monitor(
    monitor: &Arc<ProcessMonitor>,
    id: ProcessId,
    permit: ObservationPermit,
) -> Result<ObservationSession> {
    use crate::privileged_helper::process::snapshot::{
        bootstrap_sampler, create_sampler, latest_sampler, mapping_snapshots_sampler, poll_sampler,
    };

    let (mut target, mut _previous_sample) = capture_target(id)?;
    // Cache structural mappings for the sampler, updating them at each map refresh.
    let mut structural_maps: Vec<MemoryMap> =
        target.maps.iter().map(|m| m.mapping.clone()).collect();
    let mut sampler = create_sampler(id);
    if let Err(error) = bootstrap_sampler(&mut sampler, &structural_maps) {
        target.sampling_error = Some(format!("{error:#}"));
    }
    target.live_samples = latest_sampler(&sampler);
    target.mapping_snapshots = mapping_snapshots_sampler(&sampler);
    let (tx, receiver) = watch::channel(target.clone());
    let state = monitor.clone();
    let cancelled = permit.cancelled.clone();
    let mut workers = monitor.workers.lock().unwrap();
    ensure!(!process_monitor_is_stopped(monitor), "Server is stopping");
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
                    && !process_monitor_is_stopped(&state)
                    && !cancelled.load(Ordering::Relaxed)
                {
                    match poll_sampler(&mut sampler, &structural_maps) {
                        Ok(()) => target.sampling_error = None,
                        Err(e) => target.sampling_error = Some(e.to_string()),
                    }
                    std::thread::sleep(
                        Duration::from_millis(50)
                            .min(state.interval.saturating_sub(start.elapsed())),
                    );
                }
                if process_monitor_is_stopped(&state)
                    || cancelled.load(Ordering::Relaxed)
                    || tx.is_closed()
                {
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
                target.live_samples = latest_sampler(&sampler);
                target.mapping_snapshots = mapping_snapshots_sampler(&sampler);
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

pub(in crate::privileged_helper) fn stop_process_monitor(state: &ProcessMonitor) {
    let _workers = state.workers.lock().unwrap();
    state.stopped.store(true, Ordering::Relaxed);
}

pub(in crate::privileged_helper) fn process_monitor_is_stopped(state: &ProcessMonitor) -> bool {
    state.stopped.load(Ordering::Relaxed)
}

pub(in crate::privileged_helper) fn join_process_collectors(state: &ProcessMonitor) -> Result<()> {
    let workers = std::mem::take(&mut *state.workers.lock().unwrap());
    let mut failed = false;
    for worker in workers {
        failed |= worker.join().is_err();
    }
    ensure!(!failed, "process collector thread panicked");
    Ok(())
}

fn capture_target(id: ProcessId) -> Result<(ProcessDetailSnapshot, ProcessSample)> {
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
            target.maps_captured_at = Some(crate::privileged_helper::timestamp_ms());
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
