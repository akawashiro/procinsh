use super::files::FileActivity;
use super::model::{CpuActivity, IpcActivity, SensorState, SystemMonitorStatus};
use super::{
    SystemSnapshot, files::FileActivityCollector, ipc::IpcActivityCollector,
    sched::CpuActivityCollector,
};

/// Owns independently initialized BPF sensors, without service lifecycle or delivery.
pub(super) struct ActivityCollector {
    ipc: Option<IpcActivityCollector>,
    scheduler: Option<CpuActivityCollector>,
    files: Option<FileActivityCollector>,
    status: SystemMonitorStatus,
}

pub(super) struct ActivityBatch {
    pub(super) files: Vec<FileActivity>,
    pub(super) ipc: Vec<IpcActivity>,
    pub(super) cpu: Vec<CpuActivity>,
}

impl ActivityCollector {
    pub(super) fn new() -> Self {
        let mut status = SystemMonitorStatus {
            ipc: SensorState::Starting,
            cpu: SensorState::Starting,
            coverage: Some(
                "pipe read/write; socket send/recv. splice, sendfile and some io_uring paths are not observed; worker attribution is excluded.",
            ),
            files_coverage: Some(super::files::COVERAGE),
            ..SystemMonitorStatus::default()
        };
        let ipc = initialize(IpcActivityCollector::new(), &mut status.ipc);
        let scheduler = initialize(CpuActivityCollector::new(), &mut status.cpu);
        let files = initialize(FileActivityCollector::new(), &mut status.files);
        Self {
            ipc,
            scheduler,
            files,
            status,
        }
    }

    pub(super) fn poll(&mut self, snapshot: &SystemSnapshot) {
        update_state(
            self.files.as_ref().map(FileActivityCollector::poll),
            &mut self.status.files,
        );
        update_state(
            self.ipc.as_mut().map(|sensor| sensor.poll(snapshot)),
            &mut self.status.ipc,
        );
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
                update_state(Some(Ok(())), &mut self.status.cpu);
                activity
            }
            Some(Err(error)) => {
                update_state(Some(Err(error)), &mut self.status.cpu);
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
        ActivityBatch { files, ipc, cpu }
    }

    /// Sensor health only; the service sets `active` on the returned status.
    /// Returns sensor health; the service supplies the monitoring `active` flag.
    pub(super) fn status(&self) -> SystemMonitorStatus {
        self.status.clone()
    }
}

fn initialize<T>(result: anyhow::Result<T>, state: &mut SensorState) -> Option<T> {
    match result {
        Ok(sensor) => {
            *state = SensorState::Observing;
            Some(sensor)
        }
        Err(error) => {
            *state = SensorState::Unavailable(format!("{error:#}"));
            None
        }
    }
}

fn update_state(result: Option<anyhow::Result<()>>, state: &mut SensorState) {
    if let Some(result) = result {
        *state = match result {
            Ok(()) => SensorState::Observing,
            Err(error) => SensorState::Error(format!("{error:#}")),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libbpf_rs::ObjectBuilder;

    #[test]
    fn sensor_failures_are_independent_and_polling_can_recover() {
        let mut ipc = SensorState::Starting;
        let mut cpu = SensorState::Starting;
        assert!(initialize::<()>(Err(anyhow::anyhow!("load denied")), &mut ipc).is_none());
        assert!(initialize(Ok(()), &mut cpu).is_some());
        assert_eq!(ipc, SensorState::Unavailable("load denied".into()));
        assert_eq!(cpu, SensorState::Observing);
        update_state(None, &mut ipc);
        assert_eq!(ipc, SensorState::Unavailable("load denied".into()));
        update_state(Some(Err(anyhow::anyhow!("read failed"))), &mut cpu);
        assert_eq!(cpu, SensorState::Error("read failed".into()));
        update_state(Some(Ok(())), &mut cpu);
        assert_eq!(cpu, SensorState::Observing);
        assert_eq!(ipc, SensorState::Unavailable("load denied".into()));
    }

    #[test]
    fn missing_sensors_preserve_health_and_produce_empty_batches() {
        let mut collector = ActivityCollector {
            ipc: None,
            scheduler: None,
            files: None,
            status: SystemMonitorStatus {
                ipc: SensorState::Unavailable("ipc denied".into()),
                cpu: SensorState::Unavailable("cpu denied".into()),
                files: SensorState::Unavailable("files denied".into()),
                ..SystemMonitorStatus::default()
            },
        };
        let snapshot = SystemSnapshot::default();
        collector.poll(&snapshot);
        let batch = collector.drain(123, &snapshot);
        assert!(batch.ipc.is_empty() && batch.cpu.is_empty() && batch.files.is_empty());
        let mut status = collector.status();
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
        assert_eq!(sched_hooks.len(), 2);
        for name in ["schedule", "process_exit"] {
            assert!(sched_hooks.iter().any(|hook| hook == name));
            assert!(!ipc_hooks.iter().any(|hook| hook == name));
        }
        assert_eq!(ipc_hooks.len(), 4);
        for name in ["anon_pipe_read", "anon_pipe_write", "send", "recv"] {
            assert!(ipc_hooks.iter().any(|hook| hook == name));
        }
    }
}
