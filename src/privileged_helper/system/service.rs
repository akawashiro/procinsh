//! Manage system monitoring subscriptions, shared snapshots and collector workers.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`SubscribeError`] | `pub(in crate::privileged_helper)` | `enum SubscribeError` |
//! | [`SystemMonitorEvent`] | `pub(in crate::privileged_helper)` | `enum SystemMonitorEvent` |
//! | [`Subscription`] | `pub(in crate::privileged_helper)` | `struct Subscription` |
//! | [`SystemMonitor`] | `pub(in crate::privileged_helper)` | `struct SystemMonitor` |
//! | [`stopped_system_monitor`] | `pub(in crate::privileged_helper)` | `fn stopped_system_monitor(state: &`[`SystemMonitor`]`) -> bool` |
//! | [`stop_system_monitor`] | `pub(in crate::privileged_helper)` | `fn stop_system_monitor(state: &`[`SystemMonitor`]`)` |
//! | [`join_system_monitor_workers`] | `pub(in crate::privileged_helper)` | `fn join_system_monitor_workers( state: &`[`SystemMonitor`]`, ) -> `[`anyhow::Result`]`<()>` |
//! | [`active_system_monitor`] | `pub(in crate::privileged_helper)` | `fn active_system_monitor(state: &`[`SystemMonitor`]`) -> bool` |
//! | [`subscribe_system_monitor`] | `pub(in crate::privileged_helper)` | `fn subscribe_system_monitor( state: &`[`Arc`]`<`[`SystemMonitor`]`>, ) -> `[`Result`]`<`[`Subscription`]`, `[`SubscribeError`]`>` |
//! | [`snapshot_system_monitor`] | `pub(in crate::privileged_helper)` | `fn snapshot_system_monitor( state: &`[`SystemMonitor`]`, ) -> `[`Arc`]`<`[`SystemSnapshot`]`>` |
//! | [`monotonic_ns`] | `pub(super)` | `fn monotonic_ns() -> u64` |

use crate::ipc::acquisition::{SystemActivity, SystemMonitorStatus};
use {
    super::{activity, snapshot},
    crate::ipc::model::system::SystemSnapshot,
};
#[derive(Debug, PartialEq, Eq)]
pub(in crate::privileged_helper) enum SubscribeError {
    Stopped,
    TooManySubscribers,
}
/// Events delivered by the system monitoring service.
#[derive(Clone)]
pub(in crate::privileged_helper) enum SystemMonitorEvent {
    Snapshot(std::sync::Arc<SystemSnapshot>),
    Activity(std::sync::Arc<SystemActivity>),
}
use std::{
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::broadcast;
/// Owns a monitoring registration until dropped.
pub(in crate::privileged_helper) struct Subscription {
    monitor: Arc<SystemMonitor>,
    pub(in crate::privileged_helper) receiver: broadcast::Receiver<SystemMonitorEvent>,
    pub(in crate::privileged_helper) initial: Arc<SystemSnapshot>,
}
impl Drop for Subscription {
    fn drop(&mut self) {
        *self.monitor.viewers.lock().unwrap() -= 1;
    }
}
/// Manages subscribers, collection workers, snapshots, and event delivery.
pub(in crate::privileged_helper) struct SystemMonitor {
    viewers: Mutex<usize>,
    snapshot: RwLock<Arc<crate::ipc::model::system::SystemSnapshot>>,
    events: broadcast::Sender<SystemMonitorEvent>,
    stop: AtomicBool,
    started: AtomicBool,
    workers: Mutex<Vec<std::thread::JoinHandle<()>>>,
}
impl Default for SystemMonitor {
    fn default() -> Self {
        let (events, _) = broadcast::channel(16);
        Self {
            viewers: Mutex::new(0),
            snapshot: RwLock::new(Arc::new(
                crate::ipc::model::system::SystemSnapshot::default(),
            )),
            events,
            stop: AtomicBool::new(false),
            started: AtomicBool::new(false),
            workers: Mutex::new(Vec::new()),
        }
    }
}
pub(in crate::privileged_helper) fn stopped_system_monitor(state: &SystemMonitor) -> bool {
    state.stop.load(Ordering::Relaxed)
}

pub(in crate::privileged_helper) fn stop_system_monitor(state: &SystemMonitor) {
    let _viewers = state.viewers.lock().unwrap();
    state.stop.store(true, Ordering::Relaxed);
}

pub(in crate::privileged_helper) fn join_system_monitor_workers(
    state: &SystemMonitor,
) -> anyhow::Result<()> {
    let workers = std::mem::take(&mut *state.workers.lock().unwrap());
    let mut failed = false;
    for worker in workers {
        failed |= worker.join().is_err();
    }
    anyhow::ensure!(!failed, "system monitoring worker panicked");
    Ok(())
}

pub(in crate::privileged_helper) fn active_system_monitor(state: &SystemMonitor) -> bool {
    !stopped_system_monitor(state) && *state.viewers.lock().unwrap() > 0
}

pub(in crate::privileged_helper) fn subscribe_system_monitor(
    state: &Arc<SystemMonitor>,
) -> Result<Subscription, SubscribeError> {
    let mut viewers = state.viewers.lock().unwrap();
    if stopped_system_monitor(state) {
        return Err(SubscribeError::Stopped);
    }
    if *viewers >= 32 {
        return Err(SubscribeError::TooManySubscribers);
    }
    *viewers += 1;
    let viewer = Subscription {
        monitor: state.clone(),
        receiver: state.events.subscribe(),
        initial: state.snapshot.read().unwrap().clone(),
    };
    start_system_monitor(state);
    drop(viewers);
    Ok(viewer)
}

fn send_system_monitor(state: &SystemMonitor, event: SystemMonitorEvent) {
    let _ = state.events.send(event);
}

pub(in crate::privileged_helper) fn snapshot_system_monitor(
    state: &SystemMonitor,
) -> Arc<SystemSnapshot> {
    state.snapshot.read().unwrap().clone()
}

fn run_activity_system_monitor(state: &SystemMonitor) {
    use crate::privileged_helper::system::{
        activity::{
            create_activity_collector, drain_activity_collector, poll_activity_collector,
            status_activity_collector,
        },
        status::observe_status_log,
    };

    let mut collector: Option<activity::ActivityCollector> = None;
    let mut last = Instant::now();
    let mut logged = super::StatusLog::default();
    log::info!("System activity worker started");
    while !stopped_system_monitor(state) {
        if !active_system_monitor(state) {
            observe_status_log(&mut logged, &SystemMonitorStatus::default());
            collector = None;
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        let collector = collector.get_or_insert_with(|| {
            log::info!("System observation active");
            create_activity_collector()
        });
        let snapshot = snapshot_system_monitor(state);
        poll_activity_collector(&mut *collector, &snapshot);
        if last.elapsed() >= Duration::from_secs(1) {
            let batch = drain_activity_collector(&mut *collector, monotonic_ns(), &snapshot);
            let mut status = status_activity_collector(collector);
            status.active = true;
            send_system_monitor(
                state,
                SystemMonitorEvent::Activity(Arc::new(SystemActivity {
                    captured_at: crate::privileged_helper::process::timestamp_ms(),
                    window_ms: last.elapsed().as_millis() as u64,
                    files: Vec::new(),
                    ipc: Vec::new(),
                    cpu: Vec::new(),
                    signals: Vec::new(),
                    raw_activity: Some(crate::ipc::activity::RawActivity {
                        files: batch.files,
                        ipc: batch.ipc,
                        cpu: batch.cpu,
                        signals: batch.signals,
                    }),
                    status,
                })),
            );
            last = Instant::now();
        }
        observe_status_log(&mut logged, &status_activity_collector(collector));
        std::thread::sleep(Duration::from_millis(10));
    }
    log::info!("System activity worker stopped");
}

fn start_system_monitor(state: &Arc<SystemMonitor>) {
    if state.started.swap(true, Ordering::SeqCst) {
        return;
    }
    let monitor = state.clone();
    let mut workers = state.workers.lock().unwrap();
    workers.push(spawn_worker("activity", move || {
        run_activity_system_monitor(&monitor)
    }));
    let monitor = state.clone();
    workers.push(spawn_worker("snapshot", move || {
        log::info!("System snapshot worker started");
        let mut previous_warnings = Vec::new();
        let mut full = Instant::now() - Duration::from_secs(10);
        while !stopped_system_monitor(&monitor) {
            if !active_system_monitor(&monitor) {
                full = Instant::now() - Duration::from_secs(10);
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            if full.elapsed() >= Duration::from_secs(1) {
                let data = snapshot::collect_system_snapshot();
                if data.warnings != previous_warnings {
                    if data.warnings.is_empty() {
                        log::info!("System snapshot recovered");
                    } else {
                        for warning in &data.warnings {
                            log::warn!("System snapshot: {warning}");
                        }
                    }
                    previous_warnings = data.warnings.clone();
                }
                log::debug!(
                    "System snapshot collected processes={} fd_relations={} rdma_relations={}",
                    data.processes.len(),
                    data.fd_relations.len(),
                    data.rdma_relations.len()
                );
                let data = Arc::new(data);
                send_system_monitor(&monitor, SystemMonitorEvent::Snapshot(data.clone()));
                *monitor.snapshot.write().unwrap() = data;
                full = Instant::now();
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        log::info!("System snapshot worker stopped");
    }));
}

pub(super) fn monotonic_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

fn spawn_worker(
    name: &'static str,
    work: impl FnOnce() + Send + 'static,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        if let Err(panic) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
            log::error!("System {name} worker panicked");
            std::panic::resume_unwind(panic);
        }
    })
}
