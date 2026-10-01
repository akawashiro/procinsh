use super::model::{ProcessMetrics, SystemActivity, SystemMetrics, SystemMonitorStatus};
use super::{SystemSnapshot, activity, resolver, system_snapshot};
#[derive(Debug, PartialEq, Eq)]
pub(in crate::http_server) enum SubscribeError {
    Stopped,
    TooManySubscribers,
}
/// Events delivered by the system monitoring service.
#[derive(Clone)]
pub(in crate::http_server) enum SystemMonitorEvent {
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
    monitor: Arc<SystemMonitor>,
    pub(in crate::http_server) receiver: broadcast::Receiver<SystemMonitorEvent>,
    pub(in crate::http_server) initial: Arc<SystemSnapshot>,
}
impl Drop for Subscription {
    fn drop(&mut self) {
        *self.monitor.viewers.lock().unwrap() -= 1;
    }
}
/// Manages subscribers, collection workers, snapshots, and event delivery.
pub(in crate::http_server) struct SystemMonitor {
    viewers: Mutex<usize>,
    snapshot: RwLock<Arc<system_snapshot::SystemSnapshot>>,
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
            snapshot: RwLock::new(Arc::new(system_snapshot::SystemSnapshot::default())),
            events,
            stop: AtomicBool::new(false),
            started: AtomicBool::new(false),
            workers: Mutex::new(Vec::new()),
        }
    }
}
impl SystemMonitor {
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
            monitor: self.clone(),
            receiver: self.events.subscribe(),
            initial: self.snapshot.read().unwrap().clone(),
        };
        self.start();
        drop(viewers);
        Ok(viewer)
    }
    fn send(&self, event: SystemMonitorEvent) {
        let _ = self.events.send(event);
    }
    pub(in crate::http_server) fn snapshot(&self) -> Arc<SystemSnapshot> {
        self.snapshot.read().unwrap().clone()
    }
    fn run_activity(&self) {
        let mut collector: Option<activity::ActivityCollector> = None;
        let mut last = Instant::now();
        let mut logged = super::StatusLog::default();
        log::info!("System activity worker started");
        while !self.stopped() {
            if !self.active() {
                logged.observe(&SystemMonitorStatus::default());
                collector = None;
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            let collector = collector.get_or_insert_with(|| {
                log::info!("System observation active");
                activity::ActivityCollector::new()
            });
            let snapshot = self.snapshot();
            collector.poll(&snapshot);
            if last.elapsed() >= Duration::from_millis(100) {
                let batch = collector.drain(monotonic_ns(), &snapshot);
                let mut status = collector.status();
                status.active = true;
                self.send(SystemMonitorEvent::Activity(Arc::new(SystemActivity {
                    captured_at: crate::http_server::process::timestamp_ms(),
                    window_ms: last.elapsed().as_millis() as u64,
                    files: batch.files,
                    ipc: batch.ipc,
                    cpu: batch.cpu,
                    status,
                })));
                last = Instant::now();
            }
            logged.observe(&collector.status());
            std::thread::sleep(Duration::from_millis(10));
        }
        log::info!("System activity worker stopped");
    }
    fn start(self: &Arc<Self>) {
        if self.started.swap(true, Ordering::SeqCst) {
            return;
        }
        let monitor = self.clone();
        let mut workers = self.workers.lock().unwrap();
        workers.push(spawn_worker("activity", move || monitor.run_activity()));
        let monitor = self.clone();
        workers.push(spawn_worker("snapshot", move || {
            log::info!("System snapshot worker started");
            let mut previous_warnings = Vec::new();
            let mut metrics_error = None;
            let resolver = resolver::Resolver::new();
            let mut discovery = crate::http_server::process::Discovery::default();
            let mut full = Instant::now() - Duration::from_secs(10);
            let mut tick = Instant::now() - Duration::from_secs(2);
            while !monitor.stopped() {
                if !monitor.active() {
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
                    monitor.send(SystemMonitorEvent::Snapshot(data.clone()));
                    *monitor.snapshot.write().unwrap() = data;
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
                            monitor.send(SystemMonitorEvent::Metrics(SystemMetrics { processes }));
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
