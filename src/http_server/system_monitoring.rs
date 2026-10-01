//! System monitoring subscriptions and worker services.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind |
//! | --- | --- | --- |
//! | [`SubscribeError`] | `pub(super)` | `enum `[`SubscribeError`] |
//! | [`System`] | `pub(super)` | `struct `[`System`] |
//! | [`SystemEvent`] | `pub(super)` | `enum `[`SystemEvent`] |
//! | [`SystemSnapshot`] | `pub(super)` | `struct `[`SystemSnapshot`] |
//!
//! Event payloads: [`SystemEvent::Metrics`] carries [`model::SystemMetrics`]
//! (`{ processes: Vec<ProcessMetrics> }`, serialized as an array), and
//! [`SystemEvent::Activity`] carries `Arc<SystemActivity>` ([`model::SystemActivity`])
//! (`{ captured_at: u64, window_ms: u64, files: Vec<FileActivity>,
//! ipc: Vec<IpcActivity>, cpu: Vec<CpuActivity>, status: SystemStatus }`).
//! [`service::System::status`] is `pub(super) status: Mutex<SystemStatus>`.
//! [`status::StatusLog::observe`] is `pub(super) fn observe(&mut self, status: &SystemStatus)`.
//!
//! All payload types below have `pub(in crate::http_server)` visibility;
//! their fields have `pub(super)` visibility within `system_monitoring`.
//!
//! | Definition | Fields / variants |
//! | --- | --- |
//! | [`model::ProcessMetrics`] | `identity: ProcessId, cpu_percent: Option<f64>, rss_bytes: u64` |
//! | [`files::FileActivity`] | `process_id: ProcessId, resource: String, path: Option<String>, write: bool, bytes: u64, count: u64` (private fields) |
//! | [`model::IpcActivity`] | `process_id: ProcessId, resource: String, write: bool, bytes: u64, count: u64` |
//! | [`model::CpuActivity`] | `process_id: ProcessId, runtime_ns: u64, switches: u64, running_threads: usize, cpus: Vec<usize>` |
//! | [`model::SystemStatus`] | `active: bool, ipc: SensorState, cpu: SensorState, files: SensorState, coverage: Option<&'static str>, files_coverage: Option<&'static str>, lost: Option<u64>, unresolved: Option<u64>, files_lost: Option<u64>` |
//! | [`model::SensorState`] | `Idle, Starting, Observing, Unavailable(String), Error(String)`; serializes as the existing state string |
//!
//! Collector interfaces (all `pub(super)`, within `system_monitoring`):
//! - [`sched::Scheduler`]: scheduling maps and previous CPU totals.
//!   [`sched::Scheduler::new`]: `fn new() -> anyhow::Result<Self>`;
//!   [`sched::Scheduler::collect`]: `fn collect(&mut self, now: u64, snapshot: &SystemSnapshot) -> anyhow::Result<Vec<CpuActivity>>`.
//! - [`ipc::Ipc`]: IPC ring buffer, pending activity and loss accounting.
//!   [`ipc::Ipc::new`]: `fn new() -> anyhow::Result<Self>`;
//!   [`ipc::Ipc::poll`]: `fn poll(&mut self, snapshot: &SystemSnapshot) -> anyhow::Result<()>`;
//!   [`ipc::Ipc::drain`]: `fn drain(&mut self) -> Vec<IpcActivity>`;
//!   [`ipc::Ipc::lost`]: `fn lost(&self) -> u64`;
//!   [`ipc::Ipc::unresolved`]: `fn unresolved(&self) -> u64`.
mod activity;
mod files;
mod ipc;
mod model;
mod resolver;
mod sched;
mod service;
mod status;
mod system_snapshot;
use service::monotonic_ns;
pub(super) use service::{SubscribeError, System, SystemEvent};
use status::StatusLog;
pub(super) use system_snapshot::SystemSnapshot;
