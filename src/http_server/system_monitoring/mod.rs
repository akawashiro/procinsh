pub(super) use topology::Topology;
#[derive(Debug, PartialEq, Eq)]
pub(super) enum SubscribeError {
    Stopped,
    TooManySubscribers,
}
#[derive(Clone)]
pub(super) enum SystemEvent {
    Topology(std::sync::Arc<Topology>),
    Metrics(serde_json::Value),
    Activity(serde_json::Value),
}
mod activity;
mod files;
mod resolver;
mod topology;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::broadcast;
/// Owns a monitoring registration until dropped.
pub(super) struct Subscription {
    system: Arc<System>,
    pub(super) receiver: broadcast::Receiver<SystemEvent>,
    pub(super) initial: Arc<Topology>,
}
impl Drop for Subscription {
    fn drop(&mut self) {
        *self.system.viewers.lock().unwrap() -= 1;
    }
}
pub(super) struct System {
    viewers: Mutex<usize>,
    status: Mutex<Value>,
    snapshot: RwLock<Arc<topology::Topology>>,
    events: broadcast::Sender<SystemEvent>,
    stop: AtomicBool,
    started: AtomicBool,
    workers: Mutex<Vec<std::thread::JoinHandle<()>>>,
}
impl Default for System {
    fn default() -> Self {
        let (events, _) = broadcast::channel(16);
        Self {
            viewers: Mutex::new(0),
            status: Mutex::new(json!({"active":false,"ipc":"idle","cpu":"idle","files":"idle"})),
            snapshot: RwLock::new(Arc::new(topology::Topology::default())),
            events,
            stop: AtomicBool::new(false),
            started: AtomicBool::new(false),
            workers: Mutex::new(Vec::new()),
        }
    }
}
impl System {
    pub(super) fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
    pub(super) fn stop(&self) {
        let _viewers = self.viewers.lock().unwrap();
        self.stop.store(true, Ordering::Relaxed);
    }
    pub(super) fn join_workers(&self) -> anyhow::Result<()> {
        let workers = std::mem::take(&mut *self.workers.lock().unwrap());
        let mut failed = false;
        for worker in workers {
            failed |= worker.join().is_err();
        }
        anyhow::ensure!(!failed, "system monitoring worker panicked");
        Ok(())
    }
    pub(super) fn active(&self) -> bool {
        !self.stopped() && *self.viewers.lock().unwrap() > 0
    }
    pub(super) fn subscribe(self: &Arc<Self>) -> Result<Subscription, SubscribeError> {
        let mut viewers = self.viewers.lock().unwrap();
        if self.stopped() {
            return Err(SubscribeError::Stopped);
        }
        if *viewers >= 32 {
            return Err(SubscribeError::TooManySubscribers);
        }
        *viewers += 1;
        let viewer = Subscription {
            system: self.clone(),
            receiver: self.events.subscribe(),
            initial: self.snapshot.read().unwrap().clone(),
        };
        self.start();
        drop(viewers);
        Ok(viewer)
    }
    fn send(&self, event: SystemEvent) {
        let _ = self.events.send(event);
    }
    pub(super) fn snapshot(&self) -> Arc<Topology> {
        self.snapshot.read().unwrap().clone()
    }
    fn start(self: &Arc<Self>) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let system = self.clone();
        let mut workers = self.workers.lock().unwrap();
        workers.push(spawn_worker("activity", move || activity::run(system)));
        let system = self.clone();
        workers.push(spawn_worker("topology", move || {
            log::info!("System topology worker started");
            let mut previous_warnings = Vec::new();
            let mut metrics_error = None;
            let resolver = resolver::Resolver::new();
            let mut discovery = crate::http_server::process::Discovery::default();
            let mut full = Instant::now() - Duration::from_secs(10);
            let mut tick = Instant::now() - Duration::from_secs(2);
            while !system.stopped() {
                if !system.active() {
                    *system.status.lock().unwrap() =
                        json!({"active":false,"ipc":"idle","cpu":"idle","files":"idle"});
                    full = Instant::now() - Duration::from_secs(10);
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                if full.elapsed() >= Duration::from_secs(5) {
                    let mut data = topology::collect(&mut discovery);
                    if data.warnings != previous_warnings {
                        if data.warnings.is_empty() {
                            log::info!("System topology recovered");
                        } else {
                            for warning in &data.warnings {
                                log::warn!("System topology: {warning}");
                            }
                        }
                        previous_warnings = data.warnings.clone();
                    }
                    log::debug!(
                        "System topology collected nodes={} edges={}",
                        data.nodes.len(),
                        data.edges.len()
                    );
                    for edge in &mut data.edges {
                        if let Some(socket) = &mut edge.socket
                            && socket.network_peer
                        {
                            socket.remote_hostname =
                                socket.remote.and_then(|a| resolver.lookup(a.ip()));
                        }
                    }
                    let data = Arc::new(data);
                    system.send(SystemEvent::Topology(data.clone()));
                    *system.snapshot.write().unwrap() = data;
                    full = Instant::now();
                    tick = Instant::now();
                } else if tick.elapsed() >= Duration::from_secs(1) {
                    match discovery.collect() {
                        Ok(summaries) => {
                            if metrics_error.take().is_some() {
                                log::info!("System metrics recovered");
                            }
                            let metrics:Vec<_>=summaries.iter().map(|s|json!({"identity":s.identity,"cpu_percent":s.cpu_percent,"rss_bytes":s.rss_bytes})).collect();
                            system.send(SystemEvent::Metrics(json!(metrics)));
                        }
                        Err(error) => {
                            let error = format!("{error:#}");
                            if metrics_error.as_ref() != Some(&error) {
                                log::warn!("System metrics: {error}");
                            }
                            metrics_error = Some(error);
                        }
                    }
                    tick = Instant::now();
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            log::info!("System topology worker stopped");
        }));
    }
}
pub(super) fn monotonic_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) };
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}

/// Remembers only operational states, not continuously changing counters.
#[derive(Default)]
struct StatusLog(std::collections::HashMap<&'static str, String>);
impl StatusLog {
    fn changes(&mut self, status: &serde_json::Value) -> Vec<(&'static str, String)> {
        let mut changes = Vec::new();
        for key in ["ipc", "cpu", "files"] {
            if let Some(value) = status[key].as_str()
                && self.0.get(key).is_none_or(|old| old != value)
            {
                self.0.insert(key, value.to_owned());
                changes.push((key, value.to_owned()));
            }
        }
        changes
    }
    fn observe(&mut self, status: &serde_json::Value) {
        for (key, value) in self.changes(status) {
            if value.starts_with("unavailable:") || value.starts_with("error:") {
                log::warn!("System {key}: {value}");
            } else {
                log::info!("System {key}: {value}");
            }
        }
    }
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

#[cfg(test)]
mod logging_tests {
    use super::*;
    #[test]
    fn logs_changes_recovery_and_recurrence_without_repeating_errors() {
        let mut log = StatusLog::default();
        let error = json!({"cpu":"unavailable: permission denied", "lost":1});
        assert_eq!(log.changes(&error).len(), 1);
        assert!(log.changes(&error).is_empty());
        assert!(
            log.changes(&json!({"cpu":"unavailable: permission denied", "lost":2}))
                .is_empty()
        );
        assert_eq!(
            log.changes(&json!({"cpu":"unavailable: unsupported"}))
                .len(),
            1
        );
        assert_eq!(log.changes(&json!({"cpu":"observing"})).len(), 1);
        assert_eq!(log.changes(&error).len(), 1);
    }
}
