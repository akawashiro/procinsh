use super::privsep::MonitorClient;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

pub(super) struct AppState {
    pub(super) client: MonitorClient,
    pub(super) interval: Duration,
    stopped: AtomicBool,
    #[cfg(test)]
    monitor: std::sync::Arc<super::privsep::MonitorState>,
}
impl AppState {
    pub(super) fn remote(interval: Duration, client: MonitorClient) -> Self {
        Self {
            client,
            interval,
            stopped: AtomicBool::new(false),
            #[cfg(test)]
            monitor: std::sync::Arc::new(super::privsep::MonitorState::new(interval)),
        }
    }
    #[cfg(test)]
    pub(super) fn new(interval: Duration) -> Self {
        let monitor = std::sync::Arc::new(super::privsep::MonitorState::new(interval));
        Self {
            client: MonitorClient::local(monitor.clone()),
            interval,
            stopped: AtomicBool::new(false),
            monitor,
        }
    }
    pub(super) fn stopped(&self) -> bool {
        self.stopped.load(Ordering::Relaxed)
    }
    pub(super) fn stop(&self) {
        self.stopped.store(true, Ordering::Relaxed);
        #[cfg(test)]
        self.monitor.stop();
    }
    #[cfg(test)]
    pub(super) fn join_collectors(&self) -> anyhow::Result<()> {
        self.monitor.join_collectors()
    }
}
#[cfg(test)]
impl std::ops::Deref for AppState {
    type Target = super::privsep::MonitorState;
    fn deref(&self) -> &Self::Target {
        &self.monitor
    }
}
