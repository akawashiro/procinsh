//! Poll independent activity sensors and combine their bounded observations.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`ActivityCollector`] | `pub(super)` | `struct ActivityCollector` |
//! | [`ActivityBatch`] | `pub(super)` | `struct ActivityBatch` |
//! | [`create_activity_collector`] | `pub(super)` | `fn create_activity_collector() -> `[`ActivityCollector`] |
//! | [`poll_activity_collector`] | `pub(super)` | `fn poll_activity_collector(state: &mut `[`ActivityCollector`]`, snapshot: &`[`SystemSnapshot`]`)` |
//! | [`drain_activity_collector`] | `pub(super)` | `fn drain_activity_collector( state: &mut `[`ActivityCollector`]`, now_ns: u64, snapshot: &`[`SystemSnapshot`]`, ) -> `[`ActivityBatch`] |
//! | [`status_activity_collector`] | `pub(super)` | `fn status_activity_collector(state: &`[`ActivityCollector`]`) -> `[`SystemMonitorStatus`] |

use super::signals::SignalCollector;
use crate::ipc::acquisition::{SensorState, SystemMonitorStatus};
use crate::ipc::activity::FileActivity;
use {
    super::{files::FileActivityCollector, ipc::IpcActivityCollector, sched::CpuActivityCollector},
    crate::ipc::model::system::SystemSnapshot,
};

/// Owns independently initialized BPF sensors, without service lifecycle or delivery.
pub(super) struct ActivityCollector {
    ipc: Option<IpcActivityCollector>,
    scheduler: Option<CpuActivityCollector>,
    files: Option<FileActivityCollector>,
    signals: Option<SignalCollector>,
    status: SystemMonitorStatus,
}

pub(super) struct ActivityBatch {
    pub(super) files: Vec<FileActivity>,
    pub(super) ipc: Vec<crate::ipc::activity::IpcEvent>,
    pub(super) cpu: crate::ipc::activity::CpuCounters,
    pub(super) signals: Vec<crate::ipc::activity::SignalSample>,
}

pub(super) fn create_activity_collector() -> ActivityCollector {
    use crate::privileged_helper::system::{
        files::create_file_activity_collector, ipc::create_ipc_activity_collector,
        sched::create_cpu_activity_collector, signals::create_signal_collector,
    };

    let (ipc, ipc_state) = initialize(create_ipc_activity_collector());
    let (scheduler, cpu_state) = initialize(create_cpu_activity_collector());
    let (files, files_state) = initialize(create_file_activity_collector());
    let (signals, signals_state) = if std::env::var_os("PROCINSH_DISABLE_SIGNALS").is_some() {
        (
            None,
            SensorState::Unavailable("disabled by PROCINSH_DISABLE_SIGNALS".into()),
        )
    } else {
        initialize(create_signal_collector())
    };
    let status = SystemMonitorStatus {
            ipc: ipc_state,
            cpu: cpu_state,
            files: files_state,
            signals: signals_state,
            coverage: Some(
                "pipe read/write; socket send/recv. splice, sendfile and some io_uring paths are not observed; worker attribution is excluded.".into(),
            ),
            files_coverage: Some(super::files::COVERAGE.into()),
            ..SystemMonitorStatus::default()
        };
    ActivityCollector {
        signals,
        ipc,
        scheduler,
        files,
        status,
    }
}

pub(super) fn poll_activity_collector(state: &mut ActivityCollector, snapshot: &SystemSnapshot) {
    use crate::privileged_helper::system::{
        files::poll_file_activity_collector, ipc::poll_ipc_activity_collector,
        signals::poll_signal_collector,
    };

    if let Some(sensor) = state.signals.as_ref() {
        state.status.signals = update_state(poll_signal_collector(sensor));
    }
    if let Some(sensor) = state.files.as_ref() {
        state.status.files = update_state(poll_file_activity_collector(sensor));
    }
    if let Some(sensor) = state.ipc.as_mut() {
        state.status.ipc = update_state(poll_ipc_activity_collector(&mut *sensor, snapshot));
    }
}

pub(super) fn drain_activity_collector(
    state: &mut ActivityCollector,
    now_ns: u64,
    snapshot: &SystemSnapshot,
) -> ActivityBatch {
    use crate::privileged_helper::system::{
        files::{drain_file_activity_collector, lost_file_activity_collector},
        ipc::{
            drain_ipc_activity_collector, lost_ipc_activity_collector,
            unresolved_ipc_activity_collector,
        },
        sched::collect_cpu_activity_collector,
        signals::{drain_signal_collector, lost_signal_collector},
    };

    let ipc = state
        .ipc
        .as_mut()
        .map_or_else(Vec::new, drain_ipc_activity_collector);
    let result = state
        .scheduler
        .as_mut()
        .map(|sensor| collect_cpu_activity_collector(&mut *sensor, now_ns, snapshot));
    let cpu = match result {
        Some(Ok(activity)) => {
            state.status.cpu = update_state(Ok(()));
            activity
        }
        Some(Err(error)) => {
            state.status.cpu = update_state(Err(error));
            Default::default()
        }
        None => Default::default(),
    };
    state.status.lost = Some(state.ipc.as_ref().map_or(0, lost_ipc_activity_collector));
    state.status.unresolved = Some(
        state
            .ipc
            .as_ref()
            .map_or(0, unresolved_ipc_activity_collector),
    );
    state.status.files_lost = Some(state.files.as_ref().map_or(0, lost_file_activity_collector));
    let files = state
        .files
        .as_ref()
        .map_or_else(Vec::new, drain_file_activity_collector);
    state.status.signals_lost = Some(state.signals.as_ref().map_or(0, lost_signal_collector));
    let signals = state
        .signals
        .as_ref()
        .map_or_else(Vec::new, |sensor| drain_signal_collector(sensor, snapshot));
    ActivityBatch {
        files,
        ipc,
        cpu,
        signals,
    }
}

/// Returns sensor health; the service supplies the monitoring `active` flag.
pub(super) fn status_activity_collector(state: &ActivityCollector) -> SystemMonitorStatus {
    state.status.clone()
}

fn initialize<T>(result: anyhow::Result<T>) -> (Option<T>, SensorState) {
    match result {
        Ok(sensor) => (Some(sensor), SensorState::Observing),
        Err(error) => (None, SensorState::Unavailable(format!("{error:#}"))),
    }
}

fn update_state(result: anyhow::Result<()>) -> SensorState {
    match result {
        Ok(()) => SensorState::Observing,
        Err(error) => SensorState::Error(format!("{error:#}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialization_failures_and_polling_recovery_report_health() {
        let (ipc_sensor, ipc) = initialize::<()>(Err(anyhow::anyhow!("load denied")));
        let (cpu_sensor, mut cpu) = initialize(Ok(()));
        assert!(ipc_sensor.is_none());
        assert!(cpu_sensor.is_some());
        assert_eq!(ipc, SensorState::Unavailable("load denied".into()));
        assert_eq!(cpu, SensorState::Observing);
        cpu = update_state(Err(anyhow::anyhow!("read failed")));
        assert_eq!(cpu, SensorState::Error("read failed".into()));
        cpu = update_state(Ok(()));
        assert_eq!(cpu, SensorState::Observing);
    }

    #[test]
    fn missing_sensors_preserve_health_and_produce_empty_batches() {
        let mut collector = ActivityCollector {
            signals: None,
            ipc: None,
            scheduler: None,
            files: None,
            status: SystemMonitorStatus {
                ipc: SensorState::Unavailable("ipc denied".into()),
                cpu: SensorState::Unavailable("cpu denied".into()),
                files: SensorState::Unavailable("files denied".into()),
                signals: SensorState::Unavailable("signals denied".into()),
                ..SystemMonitorStatus::default()
            },
        };
        let snapshot = SystemSnapshot::default();
        super::poll_activity_collector(&mut collector, &snapshot);
        let batch = super::drain_activity_collector(&mut collector, 123, &snapshot);
        assert!(batch.signals.is_empty());
        assert!(batch.ipc.is_empty() && batch.cpu.current.is_empty() && batch.files.is_empty());
        let status = super::status_activity_collector(&collector);
        assert_eq!(
            status.signals,
            SensorState::Unavailable("signals denied".into())
        );
        assert_eq!(status.signals_lost, Some(0));
        assert_eq!(status.ipc, SensorState::Unavailable("ipc denied".into()));
        assert_eq!(status.cpu, SensorState::Unavailable("cpu denied".into()));
        assert_eq!(
            status.files,
            SensorState::Unavailable("files denied".into())
        );
        assert_eq!(
            (status.lost, status.unresolved, status.files_lost),
            (Some(0), Some(0), Some(0))
        );
    }
}
