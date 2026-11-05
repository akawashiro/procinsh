use super::{client::MonitorClient, hosts::HostRegistry};

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

pub(super) struct AppState {
    pub(super) client: MonitorClient,
    pub(super) interval: Duration,
    pub(super) hosts: Arc<HostRegistry>,
    stopped: AtomicBool,
}
impl AppState {
    pub(super) fn remote(
        interval: Duration,
        client: MonitorClient,
        hosts: Arc<HostRegistry>,
    ) -> Self {
        Self {
            client,
            interval,
            hosts,
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
