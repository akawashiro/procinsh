pub mod decode;
mod event;
mod ring;
pub mod store;
mod worker;

use crate::{process::ProcessId, symbol::Symbolizer};
use anyhow::{Result, ensure};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::JoinHandle,
};
use store::{Samples, Store};

#[derive(Clone, Copy)]
pub struct Config {
    pub hz: u32,
    pub callchain: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            hz: 49,
            callchain: true,
        }
    }
}
pub fn mono_ns() -> u64 {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts);
    }
    ts.tv_sec as u64 * 1_000_000_000 + ts.tv_nsec as u64
}
struct Shared {
    stop: AtomicBool,
    store: Mutex<Store>,
}
struct Entry {
    shared: Arc<Shared>,
    viewers: usize,
    worker: Option<JoinHandle<()>>,
}
#[derive(Default)]
struct Inner {
    stopped: bool,
    entries: HashMap<ProcessId, Entry>,
}
pub struct PerfManager {
    inner: Mutex<Inner>,
    config: Config,
}
pub struct Subscription {
    manager: Arc<PerfManager>,
    id: ProcessId,
    shared: Arc<Shared>,
}
impl PerfManager {
    pub fn new(config: Config) -> Self {
        Self {
            inner: Mutex::new(Inner::default()),
            config,
        }
    }
    pub fn subscribe(
        self: &Arc<Self>,
        id: ProcessId,
        symbols: Arc<Mutex<Symbolizer>>,
    ) -> Result<Subscription> {
        ensure!(
            (1..=199).contains(&self.config.hz),
            "sample frequency must be 1..=199 Hz"
        );
        crate::process::check_identity(id)?;
        let mut inner = self.inner.lock().unwrap();
        ensure!(!inner.stopped, "Server is stopping");
        if let Some(e) = inner.entries.get_mut(&id) {
            e.viewers += 1;
            return Ok(Subscription {
                manager: self.clone(),
                id,
                shared: e.shared.clone(),
            });
        }
        let shared = Arc::new(Shared {
            stop: AtomicBool::new(false),
            store: Mutex::new(Store::new(id, self.config.hz)),
        });
        let state = shared.clone();
        let config = self.config;
        let worker = std::thread::Builder::new()
            .name(format!("perf-{}", id.pid))
            .spawn(move || {
                log::info!("perf sampling started pid={}", id.pid);
                if let Err(e) = worker::run(id, config, &state, &symbols) {
                    let mut store = state.store.lock().unwrap();
                    store.data.status = "unavailable".into();
                    store.data.warnings.push(format!("{e:#}"));
                    log::warn!("perf sampling unavailable pid={}: {e:#}", id.pid);
                }
                log::info!("perf sampling stopped pid={}", id.pid);
            });
        let worker = match worker {
            Ok(handle) => Some(handle),
            Err(error) => {
                let mut store = shared.store.lock().unwrap();
                store.data.status = "unavailable".into();
                store
                    .data
                    .warnings
                    .push(format!("Could not start perf worker: {error}"));
                None
            }
        };
        inner.entries.insert(
            id,
            Entry {
                shared: shared.clone(),
                viewers: 1,
                worker,
            },
        );
        Ok(Subscription {
            manager: self.clone(),
            id,
            shared,
        })
    }
    pub fn stop(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.stopped = true;
        for e in inner.entries.values() {
            e.shared.stop.store(true, Ordering::Release);
        }
    }
    pub fn join(&self) {
        let mut inner = self.inner.lock().unwrap();
        for (_, e) in inner.entries.drain() {
            e.shared.stop.store(true, Ordering::Release);
            if let Some(worker) = e.worker {
                let _ = worker.join();
            }
        }
    }
    pub fn sampler_count(&self) -> usize {
        self.inner.lock().unwrap().entries.len()
    }
}
impl Subscription {
    pub fn view(&self) -> Samples {
        self.shared.store.lock().unwrap().view()
    }
}
impl Drop for Subscription {
    fn drop(&mut self) {
        let mut inner = self.manager.inner.lock().unwrap();
        if let Some(e) = inner.entries.get_mut(&self.id) {
            e.viewers -= 1;
            if e.viewers == 0 {
                let e = inner.entries.remove(&self.id).unwrap();
                e.shared.stop.store(true, Ordering::Release);
                if let Some(worker) = e.worker {
                    let _ = worker.join();
                }
            }
        }
    }
}
impl Drop for PerfManager {
    fn drop(&mut self) {
        self.stop();
        self.join();
    }
}
