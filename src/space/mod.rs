mod activity;
mod files;
pub mod http;
mod resolver;
pub mod topology;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::Read,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::broadcast;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseRequest {
    pub token: Option<String>,
    pub density: u8,
    pub selected_process: Option<crate::process::ProcessId>,
}
#[derive(Serialize)]
pub struct LeaseResponse {
    pub token: String,
    pub density: u8,
    pub expires_in: u32,
}
struct Lease {
    density: u8,
    selected_process: Option<crate::process::ProcessId>,
    updated: Instant,
}
pub struct Space {
    leases: Mutex<HashMap<String, Lease>>,
    pub status: Mutex<Value>,
    pub snapshot: RwLock<Arc<topology::Topology>>,
    pub events: broadcast::Sender<String>,
    stop: AtomicBool,
    started: AtomicBool,
}
impl Default for Space {
    fn default() -> Self {
        let (events, _) = broadcast::channel(16);
        Self {
            leases: Mutex::new(HashMap::new()),
            status: Mutex::new(
                json!({"active":false,"ipc":"idle","memory":"idle","cpu":"idle","files":"idle"}),
            ),
            snapshot: RwLock::new(Arc::new(topology::Topology::default())),
            events,
            stop: AtomicBool::new(false),
            started: AtomicBool::new(false),
        }
    }
}
impl Space {
    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
    pub fn density(&self) -> u8 {
        let mut leases = self.leases.lock().unwrap();
        leases.retain(|_, l| l.updated.elapsed() < Duration::from_secs(30));
        leases.values().map(|l| l.density).max().unwrap_or(0)
    }
    pub fn memory_targets(&self) -> std::collections::HashSet<crate::process::ProcessId> {
        let mut leases = self.leases.lock().unwrap();
        leases.retain(|_, l| l.updated.elapsed() < Duration::from_secs(30));
        leases.values().filter_map(|l| l.selected_process).collect()
    }
    pub fn lease(self: &Arc<Self>, request: LeaseRequest) -> anyhow::Result<LeaseResponse> {
        anyhow::ensure!(
            (1..=3).contains(&request.density),
            "density must be 1, 2 or 3"
        );
        let mut leases = self.leases.lock().unwrap();
        leases.retain(|_, l| l.updated.elapsed() < Duration::from_secs(30));
        let token = if let Some(token) = request.token {
            anyhow::ensure!(leases.contains_key(&token), "lease expired");
            token
        } else {
            anyhow::ensure!(leases.len() < 32, "Too many viewers");
            let mut bytes = [0u8; 16];
            std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
            bytes.iter().map(|b| format!("{b:02x}")).collect()
        };
        leases.insert(
            token.clone(),
            Lease {
                density: request.density,
                selected_process: request.selected_process,
                updated: Instant::now(),
            },
        );
        drop(leases);
        self.start();
        Ok(LeaseResponse {
            token,
            density: self.density(),
            expires_in: 30,
        })
    }
    pub fn release(&self, token: &str) {
        self.leases.lock().unwrap().remove(token);
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
        let space = self.clone();
        spawn_worker("activity", move || activity::run(space));
        let space = self.clone();
        spawn_worker("topology", move || {
            log::info!("SPACE topology worker started");
            let mut previous_warnings = Vec::new();
            let mut metrics_error = None;
            let resolver = resolver::Resolver::new();
            let mut discovery = crate::process::discovery::Discovery::default();
            let mut full = Instant::now() - Duration::from_secs(10);
            let mut tick = Instant::now() - Duration::from_secs(2);
            while !space.stopped() {
                if space.density() == 0 {
                    *space.status.lock().unwrap() = json!({"active":false,"ipc":"idle","memory":"idle","cpu":"idle","files":"idle"});
                    full = Instant::now() - Duration::from_secs(10);
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                if full.elapsed() >= Duration::from_secs(5) {
                    let mut data = topology::collect(&mut discovery);
                    if data.warnings != previous_warnings {
                        if data.warnings.is_empty() {
                            log::info!("SPACE topology recovered");
                        } else {
                            for warning in &data.warnings {
                                log::warn!("SPACE topology: {warning}");
                            }
                        }
                        previous_warnings = data.warnings.clone();
                    }
                    log::debug!(
                        "SPACE topology collected nodes={} edges={}",
                        data.nodes.len(),
                        data.edges.len()
                    );
                    for edge in &mut data.edges {
                        if let Some(socket) = &mut edge.socket {
                            if socket.network_peer {
                                socket.remote_hostname =
                                    socket.remote.and_then(|a| resolver.lookup(a.ip()));
                            }
                        }
                    }
                    space.send("topology", json!(data));
                    *space.snapshot.write().unwrap() = Arc::new(data);
                    full = Instant::now();
                    tick = Instant::now();
                } else if tick.elapsed() >= Duration::from_secs(1) {
                    match discovery.collect() {
                        Ok(summaries) => {
                            if metrics_error.take().is_some() {
                                log::info!("SPACE metrics recovered");
                            }
                            let metrics:Vec<_>=summaries.iter().map(|s|json!({"identity":s.identity,"cpu_percent":s.cpu_percent,"rss_bytes":s.rss_bytes})).collect();
                            space.send("metrics", json!(metrics));
                        }
                        Err(error) => {
                            let error = format!("{error:#}");
                            if metrics_error.as_ref() != Some(&error) {
                                log::warn!("SPACE metrics: {error}");
                            }
                            metrics_error = Some(error);
                        }
                    }
                    tick = Instant::now();
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            log::info!("SPACE topology worker stopped");
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn leases_validate_expire_and_negotiate() {
        let space = Space::default();
        space.leases.lock().unwrap().insert(
            "a".into(),
            Lease {
                density: 1,
                selected_process: None,
                updated: Instant::now(),
            },
        );
        space.leases.lock().unwrap().insert(
            "b".into(),
            Lease {
                density: 3,
                selected_process: None,
                updated: Instant::now(),
            },
        );
        let first = crate::process::ProcessId {
            pid: 42,
            start_time_ticks: 1,
        };
        let reused = crate::process::ProcessId {
            pid: 42,
            start_time_ticks: 2,
        };
        assert!(space.memory_targets().is_empty());
        space
            .leases
            .lock()
            .unwrap()
            .get_mut("a")
            .unwrap()
            .selected_process = Some(first);
        space
            .leases
            .lock()
            .unwrap()
            .get_mut("b")
            .unwrap()
            .selected_process = Some(reused);
        assert_eq!(
            space.memory_targets(),
            [first, reused].into_iter().collect()
        );
        assert_eq!(space.density(), 3);
        space.release("b");
        assert_eq!(space.density(), 1);
        assert_eq!(space.memory_targets(), [first].into_iter().collect());
        space.leases.lock().unwrap().get_mut("a").unwrap().updated =
            Instant::now() - Duration::from_secs(31);
        assert_eq!(space.density(), 0);
        assert!(space.memory_targets().is_empty());
    }
}

/// Remembers only operational states, not continuously changing counters.
#[derive(Default)]
struct StatusLog(std::collections::HashMap<&'static str, String>);
impl StatusLog {
    fn changes(&mut self, status: &serde_json::Value) -> Vec<(&'static str, String)> {
        let mut changes = Vec::new();
        for key in ["ipc", "cpu", "files", "memory"] {
            if let Some(value) = status[key].as_str() {
                if self.0.get(key).is_none_or(|old| old != value) {
                    self.0.insert(key, value.to_owned());
                    changes.push((key, value.to_owned()));
                }
            }
        }
        changes
    }
    fn observe(&mut self, status: &serde_json::Value) {
        for (key, value) in self.changes(status) {
            if value.starts_with("unavailable:") || value.starts_with("error:") {
                log::warn!("SPACE {key}: {value}");
            } else {
                log::info!("SPACE {key}: {value}");
            }
        }
    }
}

#[cfg(test)]
mod logging_tests {
    use super::*;
    #[test]
    fn logs_changes_recovery_and_recurrence_without_repeating_errors() {
        let mut log = StatusLog::default();
        let error = json!({"memory":"unavailable: permission denied", "lost":1});
        assert_eq!(log.changes(&error).len(), 1);
        assert!(log.changes(&error).is_empty());
        assert!(
            log.changes(&json!({"memory":"unavailable: permission denied", "lost":2}))
                .is_empty()
        );
        assert_eq!(
            log.changes(&json!({"memory":"unavailable: unsupported"}))
                .len(),
            1
        );
        assert_eq!(log.changes(&json!({"memory":"sampling"})).len(), 1);
        assert_eq!(log.changes(&error).len(), 1);
    }
}

fn spawn_worker(name: &'static str, work: impl FnOnce() + Send + 'static) {
    std::thread::spawn(move || {
        if let Err(panic) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)) {
            log::error!("SPACE {name} worker panicked");
            std::panic::resume_unwind(panic);
        }
    });
}
