//! System monitoring subscriptions and worker services.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind |
//! | --- | --- | --- |
//! | [`SubscribeError`] | `pub(super)` | `enum `[`SubscribeError`] |
//! | [`SystemMonitor`] | `pub(super)` | `struct `[`SystemMonitor`] |
//! | [`SystemMonitorEvent`] | `pub(super)` | `enum `[`SystemMonitorEvent`] |
//! | [`SystemSnapshot`] | `pub(super)` | `struct `[`SystemSnapshot`] |
//!
//! Event payloads: [`SystemMonitorEvent::Snapshot`] carries `Arc<SystemSnapshot>`;
//! [`SystemMonitorEvent::Activity`] carries `Arc<SystemActivity>` ([`model::SystemActivity`])
//! (`{ captured_at: u64, window_ms: u64, files: Vec<FileActivity>,
//! ipc: Vec<IpcActivity>, cpu: Vec<CpuActivity>, status: SystemMonitorStatus }`).
//! [`status::StatusLog::observe`] is `pub(super) fn observe(&mut self, status: &SystemMonitorStatus)`.
//!
//! All payload types below have `pub(in crate::http_server)` visibility;
//! their fields have `pub(super)` visibility within `system_monitoring`.
//!
//! | Definition | Fields / variants |
//! | --- | --- |
//! | [`files::FileActivity`] | `process_id: ProcessId, file: FileIdentity, path: Option<String>, write: bool, bytes: u64, count: u64` (private fields) |
//! | [`model::IpcActivity`] | `process_id: ProcessId, resource: IpcIdentity, write: bool, bytes: u64, count: u64` |
//! | [`model::CpuActivity`] | `process_id: ProcessId, runtime_ns: u64, switches: u64, running_threads: usize, cpus: Vec<usize>` |
//! | [`model::SystemMonitorStatus`] | `active: bool, ipc: SensorState, cpu: SensorState, files: SensorState, coverage: Option<&'static str>, files_coverage: Option<&'static str>, lost: Option<u64>, unresolved: Option<u64>, files_lost: Option<u64>` |
//! | [`model::SensorState`] | `Idle, Starting, Observing, Unavailable(String), Error(String)`; serializes as `{state, message?}`; message exists only for Unavailable/Error |
//!
//! [`service::Subscription::receiver`] is
//! `pub(in crate::http_server) receiver: broadcast::Receiver<SystemMonitorEvent>`.
//!
//! [`system_snapshot::Process`] has `pub(super)` visibility and fields:
//! `identity: ProcessId, parent_id: Option<ProcessId>, name: String, uid: Option<u32>,
//! username: Option<String>, euid: Option<u32>, effective_username: Option<String>,
//! maps: Vec<MemoryMap>, maps_epoch: u64, maps_error: Option<String>`.
//! All fields are `pub(super)`; CPU/RSS remain in [`super::process::ProcessSummary`].
//!
//! File and IPC identities are defined in [`super::resource`].
//! [`system_snapshot::FdEndpoint::resource`] is `pub(super) resource: IpcIdentity`.
//!
//! [`system_snapshot::FdEndpoint`] also has `pub(super) kind: FdKind, access: FdAccess`.
//! [`system_snapshot::SocketEndpoint`] fields (all `pub(super)`):
//! `protocol: SocketProtocol, state: SocketState, local: Option<InetAddress>, remote: Option<InetAddress>, path: Option<String>, network_peer: bool, remote_hostname: Option<String>`.
//! Socket types are defined in [`super::socket_types`].
//!
//! Collector interfaces (all `pub(super)`, within `system_monitoring`):
//! - [`activity::ActivityCollector`]: owns sensors and collection health; no service dependency.
//!   [`activity::ActivityCollector::new`]: `fn new() -> Self`;
//!   [`activity::ActivityCollector::poll`]: `fn poll(&mut self, snapshot: &SystemSnapshot)`;
//!   [`activity::ActivityCollector::drain`]: `fn drain(&mut self, now_ns: u64, snapshot: &SystemSnapshot) -> ActivityBatch`;
//!   [`activity::ActivityCollector::status`]: `fn status(&self) -> SystemMonitorStatus` (service sets `active`).
//! - [`activity::ActivityBatch`]: `files: Vec<FileActivity>, ipc: Vec<IpcActivity>, cpu: Vec<CpuActivity>`;
//!   all fields are `pub(super)`.
//! - [`sched::Scheduler`]: scheduling maps and previous CPU totals.
//!   [`sched::Scheduler::new`]: `fn new(namespace: pidns::PidNamespace) -> anyhow::Result<Self>`;
//!   [`sched::Scheduler::collect`]: `fn collect(&mut self, now: u64, snapshot: &SystemSnapshot) -> anyhow::Result<Vec<CpuActivity>>`.
//! - [`ipc::Ipc`]: IPC ring buffer, pending activity and loss accounting.
//!   [`ipc::Ipc::new`]: `fn new(namespace: pidns::PidNamespace) -> anyhow::Result<Self>`;
//!   [`ipc::Ipc::poll`]: `fn poll(&mut self, snapshot: &SystemSnapshot) -> anyhow::Result<()>`;
//!   [`ipc::Ipc::drain`]: `fn drain(&mut self) -> Vec<IpcActivity>`;
//!   [`ipc::Ipc::lost`]: `fn lost(&self) -> u64`;
//!   [`ipc::Ipc::unresolved`]: `fn unresolved(&self) -> u64`.
//! - [`files::Files::new`]: `pub(super) fn new(namespace: pidns::PidNamespace) -> anyhow::Result<Self>`.
//! - [`pidns::PidNamespace`]: `pub(super)` observer namespace identifier.
//!   [`pidns::PidNamespace::current`]: `pub(super) fn current() -> anyhow::Result<Self>`;
//!   [`pidns::PidNamespace::configure`]: `pub(super) fn configure(self, object: &libbpf_rs::Object) -> anyhow::Result<()>`.
mod activity;
mod files;
mod ipc;
mod model;
mod pidns;
mod resolver;
mod sched;
mod service;
mod status;
mod system_snapshot;
use service::monotonic_ns;
pub(super) use service::{SubscribeError, SystemMonitor, SystemMonitorEvent};
use status::StatusLog;
pub(super) use system_snapshot::SystemSnapshot;
