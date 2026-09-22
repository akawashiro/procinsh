mod activity;
mod files;
pub mod http;
mod resolver;
pub mod topology;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::broadcast;
/// Owned by an HTTP response stream, including before its first poll.
pub(super) struct Viewer {
    system: Arc<System>,
}
impl Drop for Viewer {
    fn drop(&mut self) {
        *self.system.viewers.lock().unwrap() -= 1;
    }
}
pub struct System {
    viewers: Mutex<usize>,
    pub status: Mutex<Value>,
    pub snapshot: RwLock<Arc<topology::Topology>>,
    pub events: broadcast::Sender<String>,
    stop: AtomicBool,
    started: AtomicBool,
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
        }
    }
}
impl System {
    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
    pub fn stop(&self) {
        let _viewers = self.viewers.lock().unwrap();
        self.stop.store(true, Ordering::Relaxed);
    }
    pub fn active(&self) -> bool {
        !self.stopped() && *self.viewers.lock().unwrap() > 0
    }
    pub(super) fn viewer(self: &Arc<Self>) -> Result<Viewer, axum::http::StatusCode> {
        let mut viewers = self.viewers.lock().unwrap();
        if self.stopped() {
            return Err(axum::http::StatusCode::SERVICE_UNAVAILABLE);
        }
        if *viewers >= 32 {
            return Err(axum::http::StatusCode::TOO_MANY_REQUESTS);
        }
        *viewers += 1;
        let viewer = Viewer {
            system: self.clone(),
        };
        drop(viewers);
        self.start();
        Ok(viewer)
    }
    pub fn send(&self, event: &str, data: Value) {
        let _ = self
            .events
            .send(json!({"event":event,"data":data}).to_string());
    }
    fn start(self: &Arc<Self>) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let system = self.clone();
        spawn_worker("activity", move || activity::run(system));
        let system = self.clone();
        spawn_worker("topology", move || {
            log::info!("System topology worker started");
            let mut previous_warnings = Vec::new();
            let mut metrics_error = None;
            let resolver = resolver::Resolver::new();
            let mut discovery = crate::process::discovery::Discovery::default();
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
                    system.send("topology", json!(data));
                    *system.snapshot.write().unwrap() = Arc::new(data);
                    full = Instant::now();
                    tick = Instant::now();
                } else if tick.elapsed() >= Duration::from_secs(1) {
                    match discovery.collect() {
                        Ok(summaries) => {
                            if metrics_error.take().is_some() {
                                log::info!("System metrics recovered");
                            }
                            let metrics:Vec<_>=summaries.iter().map(|s|json!({"identity":s.identity,"cpu_percent":s.cpu_percent,"rss_bytes":s.rss_bytes})).collect();
                            system.send("metrics", json!(metrics));
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
        });
    }
}
pub fn monotonic_ns() -> u64 {
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

fn spawn_worker(name: &'static str, work: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || {
        if let Err(panic) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
            log::error!("System {name} worker panicked");
            std::panic::resume_unwind(panic);
        }
    });
}

#[cfg(test)]
mod logging_tests {
    use super::*;
    #[tokio::test]
    async fn shutdown_drops_stream_registration() {
        let state = Arc::new(crate::state::AppState::new(Duration::from_secs(1)));
        let response = http::events(axum::extract::State(state.clone()))
            .await
            .unwrap();
        assert_eq!(*state.system.viewers.lock().unwrap(), 1);
        state.stop();
        tokio::time::timeout(
            Duration::from_secs(3),
            axum::body::to_bytes(response.into_body(), usize::MAX),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(*state.system.viewers.lock().unwrap(), 0);
    }

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
