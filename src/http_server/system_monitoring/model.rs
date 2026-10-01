//! Typed monitoring data; serialization preserves the existing SSE schema.
use super::files::FileActivity;
use crate::http_server::process::ProcessId;
use crate::http_server::resource::IpcIdentity;
use serde::Serialize;

/// Metrics are a JSON array on the wire, without an enclosing object.
#[derive(Clone, Serialize)]
#[serde(transparent)]
pub(in crate::http_server) struct SystemMetrics {
    pub(super) processes: Vec<ProcessMetrics>,
}
#[derive(Clone, Serialize)]
pub(in crate::http_server) struct ProcessMetrics {
    pub(super) identity: ProcessId,
    pub(super) cpu_percent: Option<f64>,
    pub(super) rss_bytes: u64,
}
#[derive(Clone, Serialize)]
pub(in crate::http_server) struct SystemActivity {
    pub(super) captured_at: u64,
    pub(super) window_ms: u64,
    pub(super) files: Vec<FileActivity>,
    pub(super) ipc: Vec<IpcActivity>,
    pub(super) cpu: Vec<CpuActivity>,
    pub(super) status: SystemMonitorStatus,
}
#[derive(Clone, Serialize)]
pub(in crate::http_server) struct IpcActivity {
    pub(super) process_id: ProcessId,
    pub(super) resource: IpcIdentity,
    pub(super) write: bool,
    pub(super) bytes: u64,
    pub(super) count: u64,
}
#[derive(Clone, Serialize)]
pub(in crate::http_server) struct CpuActivity {
    pub(super) process_id: ProcessId,
    pub(super) runtime_ns: u64,
    pub(super) switches: u64,
    pub(super) running_threads: usize,
    pub(super) cpus: Vec<usize>,
}
#[derive(Clone, Default, Serialize)]
/// State and collection health of the system monitoring sensors.
pub(in crate::http_server) struct SystemMonitorStatus {
    pub(super) active: bool,
    pub(super) ipc: SensorState,
    pub(super) cpu: SensorState,
    pub(super) files: SensorState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) coverage: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) files_coverage: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) lost: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) unresolved: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) files_lost: Option<u64>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(tag = "state", content = "message", rename_all = "snake_case")]
pub(in crate::http_server) enum SensorState {
    #[default]
    Idle,
    Starting,
    Observing,
    Unavailable(String),
    Error(String),
}
impl std::fmt::Display for SensorState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Idle => f.write_str("idle"),
            Self::Starting => f.write_str("starting"),
            Self::Observing => f.write_str("observing"),
            Self::Unavailable(message) => write!(f, "unavailable: {message}"),
            Self::Error(message) => write!(f, "error: {message}"),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn metrics_preserve_array_nullable_cpu_and_identity() {
        let metrics = SystemMetrics {
            processes: vec![
                ProcessMetrics {
                    identity: ProcessId {
                        pid: 42,
                        start_time_ticks: 123,
                    },
                    cpu_percent: None,
                    rss_bytes: 4096,
                },
                ProcessMetrics {
                    identity: ProcessId {
                        pid: 43,
                        start_time_ticks: 124,
                    },
                    cpu_percent: Some(12.5),
                    rss_bytes: 8192,
                },
            ],
        };
        assert_eq!(
            to_value(metrics).unwrap(),
            json!([
                {"identity":{"pid":42,"start_time_ticks":123},"cpu_percent":null,"rss_bytes":4096},
                {"identity":{"pid":43,"start_time_ticks":124},"cpu_percent":12.5,"rss_bytes":8192}
            ])
        );
        assert_eq!(
            to_value(SystemMetrics { processes: vec![] }).unwrap(),
            json!([])
        );
    }

    #[test]
    fn activity_preserves_fields_counters_and_structured_sensor_states() {
        let id = ProcessId {
            pid: 42,
            start_time_ticks: 123,
        };
        let activity = SystemActivity {
            captured_at: 1000,
            window_ms: 100,
            files: vec![],
            ipc: vec![IpcActivity {
                process_id: id,
                resource: IpcIdentity {
                    kind: crate::http_server::resource::IpcKind::Pipe,
                    device: crate::http_server::resource::DeviceId::from_stat(1),
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
            status: SystemMonitorStatus {
                active: true,
                ipc: SensorState::Observing,
                cpu: SensorState::Unavailable("permission denied".into()),
                files: SensorState::Error("poll failed".into()),
                coverage: Some("ipc coverage"),
                files_coverage: Some("file coverage"),
                lost: Some(0),
                unresolved: Some(1),
                files_lost: Some(2),
            },
        };
        assert_eq!(
            to_value(activity).unwrap(),
            json!({
                "captured_at":1000,"window_ms":100,"files":[],
                "ipc":[{"process_id":{"pid":42,"start_time_ticks":123},"resource":{"kind":"pipe","device":{"major":0,"minor":1},"inode":"2"},"write":true,"bytes":256,"count":2}],
                "cpu":[{"process_id":{"pid":42,"start_time_ticks":123},"runtime_ns":500,"switches":3,"running_threads":2,"cpus":[0,2]}],
                "status":{"active":true,"ipc":{"state":"observing"},"cpu":{"state":"unavailable","message":"permission denied"},"files":{"state":"error","message":"poll failed"},"coverage":"ipc coverage","files_coverage":"file coverage","lost":0,"unresolved":1,"files_lost":2}
            })
        );
    }

    #[test]
    fn idle_and_starting_preserve_absent_status_fields() {
        assert_eq!(
            to_value(SystemMonitorStatus::default()).unwrap(),
            json!({"active":false,"ipc":{"state":"idle"},"cpu":{"state":"idle"},"files":{"state":"idle"}})
        );
        assert_eq!(
            to_value(SensorState::Starting).unwrap(),
            json!({"state":"starting"})
        );
    }
}
