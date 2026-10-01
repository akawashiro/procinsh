use super::{process, system_monitoring};
use anyhow::Result;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

pub(super) struct AppState {
    pub(super) system_monitor: Arc<system_monitoring::SystemMonitor>,
    pub(super) discovery: Mutex<process::Discovery>,
    pub(super) monitoring: Arc<process::Monitoring>,
    pub(super) snapshotter: process::Snapshotter,
    pub(super) interval: Duration,
}
impl AppState {
    pub(super) fn new(interval: Duration) -> Self {
        Self {
            system_monitor: Arc::new(system_monitoring::SystemMonitor::default()),
            discovery: Mutex::new(process::Discovery::default()),
            monitoring: Arc::new(process::Monitoring::new(interval)),
            snapshotter: process::Snapshotter::default(),
            interval,
        }
    }
    pub(super) fn stop(&self) {
        self.monitoring.stop();
        self.system_monitor.stop();
    }
    pub(super) fn join_collectors(&self) -> Result<()> {
        let process_result = self.monitoring.join_collectors();
        let system_result = self.system_monitor.join_workers();
        process_result.and(system_result)
    }
}
