use super::model::{ProcessMetrics, SystemActivity, SystemMetrics, SystemStatus};
use super::{SystemSnapshot, ipc, resolver, system_snapshot};
#[derive(Debug, PartialEq, Eq)]
pub(in crate::http_server) enum SubscribeError {
    Stopped,
    TooManySubscribers,
}
#[derive(Clone)]
pub(in crate::http_server) enum SystemEvent {
    Snapshot(std::sync::Arc<SystemSnapshot>),
    Metrics(SystemMetrics),
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
pub(in crate::http_server) struct Subscription {
    system: Arc<System>,
    pub(in crate::http_server) receiver: broadcast::Receiver<SystemEvent>,
    pub(in crate::http_server) initial: Arc<SystemSnapshot>,
}
impl Drop for Subscription {
    fn drop(&mut self) {
        *self.system.viewers.lock().unwrap() -= 1;
    }
}
pub(in crate::http_server) struct System {
    viewers: Mutex<usize>,
    pub(super) status: Mutex<SystemStatus>,
    snapshot: RwLock<Arc<system_snapshot::SystemSnapshot>>,
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
            status: Mutex::new(SystemStatus::default()),
            snapshot: RwLock::new(Arc::new(system_snapshot::SystemSnapshot::default())),
            events,
            stop: AtomicBool::new(false),
            started: AtomicBool::new(false),
            workers: Mutex::new(Vec::new()),
        }
    }
}
impl System {
    pub(in crate::http_server) fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
    pub(in crate::http_server) fn stop(&self) {
        let _viewers = self.viewers.lock().unwrap();
        self.stop.store(true, Ordering::Relaxed);
    }
    pub(in crate::http_server) fn join_workers(&self) -> anyhow::Result<()> {
        let workers = std::mem::take(&mut *self.workers.lock().unwrap());
        let mut failed = false;
        for worker in workers {
            failed |= worker.join().is_err();
        }
        anyhow::ensure!(!failed, "system monitoring worker panicked");
        Ok(())
    }
    pub(in crate::http_server) fn active(&self) -> bool {
        !self.stopped() && *self.viewers.lock().unwrap() > 0
    }
    pub(in crate::http_server) fn subscribe(
        self: &Arc<Self>,
    ) -> Result<Subscription, SubscribeError> {
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
    pub(super) fn send(&self, event: SystemEvent) {
        let _ = self.events.send(event);
    }
    pub(in crate::http_server) fn snapshot(&self) -> Arc<SystemSnapshot> {
        self.snapshot.read().unwrap().clone()
    }
    fn start(self: &Arc<Self>) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let system = self.clone();
        let mut workers = self.workers.lock().unwrap();
        workers.push(spawn_worker("ipc", move || ipc::run(system)));
        let system = self.clone();
        workers.push(spawn_worker("snapshot", move || {
            log::info!("System snapshot worker started");
            let mut previous_warnings = Vec::new();
            let mut metrics_error = None;
            let resolver = resolver::Resolver::new();
            let mut discovery = crate::http_server::process::Discovery::default();
            let mut full = Instant::now() - Duration::from_secs(10);
            let mut tick = Instant::now() - Duration::from_secs(2);
            while !system.stopped() {
                if !system.active() {
                    *system.status.lock().unwrap() = SystemStatus::default();
                    full = Instant::now() - Duration::from_secs(10);
                    std::thread::sleep(Duration::from_millis(100));
                    continue;
                }
                if full.elapsed() >= Duration::from_secs(5) {
                    let mut data = system_snapshot::collect(&mut discovery);
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
                        "System snapshot collected processes={} fd_relations={}",
                        data.processes.len(),
                        data.fd_relations.len()
                    );
                    for relation in &mut data.fd_relations {
                        if let Some(socket) = &mut relation.socket
                            && socket.network_peer
                        {
                            socket.remote_hostname =
                                socket.remote.and_then(|a| resolver.lookup(a.ip()));
                        }
                    }
                    let data = Arc::new(data);
                    system.send(SystemEvent::Snapshot(data.clone()));
                    *system.snapshot.write().unwrap() = data;
                    full = Instant::now();
                    tick = Instant::now();
                } else if tick.elapsed() >= Duration::from_secs(1) {
                    match discovery.collect() {
                        Ok(summaries) => {
                            if metrics_error.take().is_some() {
                                log::info!("System metrics recovered");
                            }
                            let processes = summaries
                                .iter()
                                .map(|s| ProcessMetrics {
                                    identity: s.identity,
                                    cpu_percent: s.cpu_percent,
                                    rss_bytes: s.rss_bytes,
                                })
                                .collect();
                            system.send(SystemEvent::Metrics(SystemMetrics { processes }));
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
            log::info!("System snapshot worker stopped");
        }));
    }
}
pub(in crate::http_server) fn monotonic_ns() -> u64 {
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
