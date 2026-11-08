use super::{process, system};
use anyhow::Result;
use std::{sync::Arc, time::Duration};

pub(super) struct AppState {
    pub(super) system_monitor: Arc<system::SystemMonitor>,
    pub(super) monitoring: Arc<process::ProcessMonitor>,
    pub(super) interval: Duration,
}
pub(super) fn create_app_state(interval: Duration) -> AppState {
    use crate::privileged_helper::process::create_process_monitor;

    AppState {
        system_monitor: Arc::new(system::SystemMonitor::default()),
        monitoring: Arc::new(create_process_monitor(interval)),
        interval,
    }
}

pub(super) fn stop_app_state(state: &AppState) {
    use crate::privileged_helper::{process::stop_process_monitor, system::stop_system_monitor};

    stop_process_monitor(&state.monitoring);
    stop_system_monitor(&state.system_monitor);
}

pub(super) fn join_collectors_app_state(state: &AppState) -> Result<()> {
    use crate::privileged_helper::{
        process::join_process_collectors, system::join_system_monitor_workers,
    };

    let process_result = join_process_collectors(&state.monitoring);
    let system_result = join_system_monitor_workers(&state.system_monitor);
    process_result.and(system_result)
}
