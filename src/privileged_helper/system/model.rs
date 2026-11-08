//! Raw system activity and sensor health data for monitoring delivery.
//!
//! # Interface
//!
//! No named types or functions are exported.

#[cfg(test)]
use crate::ipc::activity::{CpuActivity, IpcActivity};
#[cfg(test)]
use crate::ipc::model::resource::IpcIdentity;
#[cfg(test)]
use crate::ipc::schema::ProcessId;

use crate::ipc::acquisition::SensorState;
impl std::fmt::Display for SensorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => f.write_str("idle"),
            Self::Observing => f.write_str("observing"),
            Self::Unavailable(message) => write!(f, "unavailable: {message}"),
            Self::Error(message) => write!(f, "error: {message}"),
        }
    }
}
#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::ipc::acquisition::{SensorState, SystemActivity, SystemMonitorStatus},
        serde_json::{json, to_value},
    };

    #[test]
    fn activity_preserves_fields_counters_and_structured_sensor_states() {
        use crate::ipc::model::resource::device_id_from_stat;

        let id = ProcessId {
            pid: 42,
            start_time_ticks: 123,
        };
        let activity = SystemActivity {
            captured_at: 1000,
            window_ms: 100,
            files: vec![],
            signals: vec![],
            ipc: vec![IpcActivity {
                process_id: id,
                resource: IpcIdentity {
                    kind: crate::ipc::model::resource::IpcKind::Pipe,
                    device: device_id_from_stat(1),
                    inode: 2,
                },
                write: true,
                bytes: 256,
                count: 2,
            }],
            cpu: vec![CpuActivity {
                process_id: id,
                runtime_ns: 500,
                switches: 3,
                running_threads: 2,
                cpus: vec![0, 2],
            }],
            raw_activity: None,
            status: SystemMonitorStatus {
                active: true,
                signals: SensorState::Observing,
                signals_lost: Some(3),
                ipc: SensorState::Observing,
                cpu: SensorState::Unavailable("permission denied".into()),
                files: SensorState::Error("poll failed".into()),
                coverage: Some("ipc coverage".into()),
                files_coverage: Some("file coverage".into()),
                lost: Some(0),
                unresolved: Some(1),
                files_lost: Some(2),
            },
        };
        assert_eq!(
            to_value(activity).unwrap(),
            json!({
                "captured_at":1000,"window_ms":100,"files":[],"signals":[],
                "ipc":[{"process_id":{"pid":42,"start_time_ticks":123},"resource":{"kind":"pipe","device":{"major":0,"minor":1},"inode":"2"},"write":true,"bytes":256,"count":2}],
                "cpu":[{"process_id":{"pid":42,"start_time_ticks":123},"runtime_ns":500,"switches":3,"running_threads":2,"cpus":[0,2]}],
                "status":{"active":true,"signals":{"state":"observing"},"signals_lost":3,"ipc":{"state":"observing"},"cpu":{"state":"unavailable","message":"permission denied"},"files":{"state":"error","message":"poll failed"},"coverage":"ipc coverage","files_coverage":"file coverage","lost":0,"unresolved":1,"files_lost":2}
            })
        );
    }

    #[test]
    fn idle_preserves_absent_status_fields() {
        assert_eq!(
            to_value(SystemMonitorStatus::default()).unwrap(),
            json!({"active":false,"signals":{"state":"idle"},"ipc":{"state":"idle"},"cpu":{"state":"idle"},"files":{"state":"idle"}})
        );
    }
}
