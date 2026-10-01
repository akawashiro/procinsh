use super::System;
use super::model::{SensorState, SystemActivity, SystemStatus};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
pub(super) fn run(system: Arc<System>) {
    let mut ipc: Option<super::ipc::Ipc> = None;
    let mut scheduler: Option<super::sched::Scheduler> = None;
    let mut files: Option<super::files::Files> = None;
    let mut active = false;
    let mut last = Instant::now();
    let mut logged = super::StatusLog::default();
    log::info!("System activity worker started");
    while !system.stopped() {
        if !system.active() {
            logged.observe(&SystemStatus::default());
            ipc = None;
            scheduler = None;
            files = None;
            active = false;
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        if !active {
            log::info!("System observation active");
            let mut status = SystemStatus {
                active: true,
                ipc: SensorState::Starting,
                cpu: SensorState::Starting,
                coverage: Some(
                    "pipe read/write; socket send/recv. splice, sendfile and some io_uring paths are not observed; worker attribution is excluded.",
                ),
                ..SystemStatus::default()
            };
            match super::ipc::Ipc::new() {
                Ok(sensor) => {
                    ipc = Some(sensor);
                    status.ipc = SensorState::Observing;
                }
                Err(error) => status.ipc = SensorState::Unavailable(format!("{error:#}")),
            }
            match super::sched::Scheduler::new() {
                Ok(sensor) => {
                    scheduler = Some(sensor);
                    status.cpu = SensorState::Observing;
                }
                Err(error) => status.cpu = SensorState::Unavailable(format!("{error:#}")),
            }
            if files.is_none() {
                match super::files::Files::new() {
                    Ok(sensor) => {
                        files = Some(sensor);
                        status.files = SensorState::Observing;
                    }
                    Err(error) => {
                        status.files = SensorState::Unavailable(format!("{error:#}"));
                    }
                }
            } else {
                status.files = SensorState::Observing;
            }
            status.files_coverage = Some(super::files::COVERAGE);
            *system.status.lock().unwrap() = status;
            active = true;
        }
        if let Some(sensor) = &files {
            system.status.lock().unwrap().files = match sensor.poll() {
                Ok(()) => SensorState::Observing,
                Err(error) => SensorState::Error(format!("{error:#}")),
            };
        }
        let snapshot = system.snapshot();
        if let Some(sensor) = &mut ipc {
            system.status.lock().unwrap().ipc = match sensor.poll(&snapshot) {
                Ok(()) => SensorState::Observing,
                Err(error) => SensorState::Error(format!("{error:#}")),
            };
        }
        if last.elapsed() >= Duration::from_millis(100) {
            let ipc_events = ipc.as_mut().map_or_else(Vec::new, |sensor| sensor.drain());
            let cpu = if let Some(sensor) = &mut scheduler {
                match sensor.collect(super::monotonic_ns(), &snapshot) {
                    Ok(activity) => {
                        system.status.lock().unwrap().cpu = SensorState::Observing;
                        activity
                    }
                    Err(error) => {
                        system.status.lock().unwrap().cpu =
                            SensorState::Error(format!("{error:#}"));
                        Vec::new()
                    }
                }
            } else {
                Vec::new()
            };
            let mut status = system.status.lock().unwrap();
            status.lost = Some(ipc.as_ref().map_or(0, |sensor| sensor.lost()));
            status.unresolved = Some(ipc.as_ref().map_or(0, |sensor| sensor.unresolved()));
            status.files_lost = Some(files.as_ref().map_or(0, |sensor| sensor.lost()));
            let file_events = files
                .as_ref()
                .map_or_else(Vec::new, |sensor| sensor.drain());
            system.send(super::SystemEvent::Activity(Arc::new(SystemActivity {
                captured_at: crate::http_server::process::timestamp_ms(),
                window_ms: last.elapsed().as_millis() as u64,
                files: file_events,
                ipc: ipc_events,
                cpu,
                status: status.clone(),
            })));
            last = Instant::now();
        }
        logged.observe(&system.status.lock().unwrap());
        std::thread::sleep(Duration::from_millis(10));
    }
    log::info!("System activity worker stopped");
}

#[cfg(test)]
mod tests {
    use libbpf_rs::ObjectBuilder;

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
