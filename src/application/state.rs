use super::client::MonitorClient;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

pub(super) struct AppState {
    pub(super) client: MonitorClient,
    pub(super) interval: Duration,
    stopped: AtomicBool,
}
impl AppState {
    pub(super) fn remote(interval: Duration, client: MonitorClient) -> Self {
        Self {
            client,
            interval,
            stopped: AtomicBool::new(false),
        }
    }
    pub(super) fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }
    pub(super) fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
    }
}
