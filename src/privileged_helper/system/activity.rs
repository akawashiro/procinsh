use super::files::FileActivity;
use super::model::{CpuActivity, IpcActivity, SensorState, SystemMonitorStatus};
use super::{
    SystemSnapshot, files::FileActivityCollector, ipc::IpcActivityCollector,
    sched::CpuActivityCollector,
};
use super::{model::SignalEvent, signals::SignalCollector};

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
    pub(super) ipc: Vec<IpcActivity>,
    pub(super) cpu: Vec<CpuActivity>,
    pub(super) signals: Vec<SignalEvent>,
}

impl ActivityCollector {
    pub(super) fn new() -> Self {
        let (ipc, ipc_state) = initialize(IpcActivityCollector::new());
        let (scheduler, cpu_state) = initialize(CpuActivityCollector::new());
        let (files, files_state) = initialize(FileActivityCollector::new());
        let (signals, signals_state) = if std::env::var_os("PROCINSH_DISABLE_SIGNALS").is_some() {
            (
                None,
                SensorState::Unavailable("disabled by PROCINSH_DISABLE_SIGNALS".into()),
            )
        } else {
            initialize(SignalCollector::new())
        };
        let status = SystemMonitorStatus {
            ipc: ipc_state,
            cpu: cpu_state,
            files: files_state,
            signals: signals_state,
            coverage: Some(
                "pipe read/write; socket send/recv. splice, sendfile and some io_uring paths are not observed; worker attribution is excluded.",
            ),
            files_coverage: Some(super::files::COVERAGE),
            ..SystemMonitorStatus::default()
        };
        Self {
            signals,
            ipc,
            scheduler,
            files,
            status,
        }
    }

    pub(super) fn poll(&mut self, snapshot: &SystemSnapshot) {
        if let Some(sensor) = self.signals.as_ref() {
            self.status.signals = update_state(sensor.poll());
        }
        if let Some(sensor) = self.files.as_ref() {
            self.status.files = update_state(sensor.poll());
        }
        if let Some(sensor) = self.ipc.as_mut() {
            self.status.ipc = update_state(sensor.poll(snapshot));
        }
    }

    pub(super) fn drain(&mut self, now_ns: u64, snapshot: &SystemSnapshot) -> ActivityBatch {
        let ipc = self
            .ipc
            .as_mut()
            .map_or_else(Vec::new, IpcActivityCollector::drain);
        let result = self
            .scheduler
            .as_mut()
            .map(|sensor| sensor.collect(now_ns, snapshot));
        let cpu = match result {
            Some(Ok(activity)) => {
                self.status.cpu = update_state(Ok(()));
                activity
            }
            Some(Err(error)) => {
                self.status.cpu = update_state(Err(error));
                Vec::new()
            }
            None => Vec::new(),
        };
        self.status.lost = Some(self.ipc.as_ref().map_or(0, IpcActivityCollector::lost));
        self.status.unresolved = Some(
            self.ipc
                .as_ref()
                .map_or(0, IpcActivityCollector::unresolved),
        );
        self.status.files_lost = Some(self.files.as_ref().map_or(0, FileActivityCollector::lost));
        let files = self
            .files
            .as_ref()
            .map_or_else(Vec::new, FileActivityCollector::drain);
        self.status.signals_lost = Some(self.signals.as_ref().map_or(0, SignalCollector::lost));
        let signals = self
            .signals
            .as_ref()
            .map_or_else(Vec::new, |sensor| sensor.drain(snapshot));
        ActivityBatch {
            files,
            ipc,
            cpu,
            signals,
        }
    }

    /// Returns sensor health; the service supplies the monitoring `active` flag.
    pub(super) fn status(&self) -> SystemMonitorStatus {
        self.status.clone()
    }
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
    use libbpf_rs::ObjectBuilder;

    #[test]
    fn sensor_failures_are_independent_and_polling_can_recover() {
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
        assert_eq!(ipc, SensorState::Unavailable("load denied".into()));
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
        collector.poll(&snapshot);
        let batch = collector.drain(123, &snapshot);
        assert!(batch.signals.is_empty());
        assert!(batch.ipc.is_empty() && batch.cpu.is_empty() && batch.files.is_empty());
        let mut status = collector.status();
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
        status.active = true;
        assert!(!collector.status().active);
    }

    #[test]
    fn compiled_collectors_have_independent_hooks_and_maps() {
        let sched = ObjectBuilder::default()
            .open_memory(include_bytes!(concat!(env!("OUT_DIR"), "/sched.bpf.o")))
            .unwrap();
        let ipc = ObjectBuilder::default()
            .open_memory(include_bytes!(concat!(env!("OUT_DIR"), "/ipc.bpf.o")))
            .unwrap();
        let sched_maps: Vec<_> = sched.maps().map(|map| map.name().to_owned()).collect();
        let ipc_maps: Vec<_> = ipc.maps().map(|map| map.name().to_owned()).collect();
        for name in ["cpu_current", "cpu_totals"] {
            assert!(sched_maps.iter().any(|map| map == name));
            assert!(!ipc_maps.iter().any(|map| map == name));
        }
        for name in ["events", "lost"] {
            assert!(ipc_maps.iter().any(|map| map == name));
            assert!(!sched_maps.iter().any(|map| map == name));
        }
        let sched_hooks: Vec<_> = sched.progs().map(|prog| prog.name().to_owned()).collect();
        let ipc_hooks: Vec<_> = ipc.progs().map(|prog| prog.name().to_owned()).collect();
        assert_eq!(sched_hooks, ["schedule"]);
        assert!(!ipc_hooks.iter().any(|hook| hook == "schedule"));
        assert_eq!(ipc_hooks.len(), 4);
        for name in ["anon_pipe_read", "anon_pipe_write", "send", "recv"] {
            assert!(ipc_hooks.iter().any(|hook| hook == name));
        }
    }
}
