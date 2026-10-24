use super::{process, system};
use anyhow::Result;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

pub(super) struct AppState {
    pub(super) system_monitor: Arc<system::SystemMonitor>,
    pub(super) discovery: Mutex<process::ProcessScanner>,
    pub(super) monitoring: Arc<process::ProcessMonitor>,
    pub(super) interval: Duration,
}
impl AppState {
    pub(super) fn new(interval: Duration) -> Self {
        Self {
            system_monitor: Arc::new(system::SystemMonitor::default()),
            discovery: Mutex::new(process::ProcessScanner::default()),
            monitoring: Arc::new(process::ProcessMonitor::new(interval)),
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
