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
//! their fields have `pub(super)` visibility within `system`.
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
//! [`snapshot::Process`] has `pub(super)` visibility and fields:
//! `identity: ProcessId, parent_id: Option<ProcessId>, name: String, uid: Option<u32>,
//! username: Option<String>, euid: Option<u32>, effective_username: Option<String>,
//! maps: Vec<SpaceMemoryMap>, maps_epoch: u64, maps_error: Option<String>`.
//! All fields are `pub(super)`; CPU/RSS remain in [`super::process::ProcessSummary`].
//!
//! [`snapshot::SpaceMemoryMap`] is `pub(super)` with private fields:
//! `start: u64, end: u64, readable: bool, writable: bool, executable: bool,
//! private: bool, file_offset: u64, device: DeviceId, inode: u64, pathname: Option<String>`.
//! It excludes RSS/PSS; addresses are hex strings and inode is a decimal string.
//! [`SnapshotDelivery`] re-exports [`delivery::SnapshotDelivery`] as `pub(super)`.
//! [`delivery::SnapshotDelivery::encode`] is `pub(in crate::http_server)`:
//! `fn encode(&mut self, snapshot: Arc<SystemSnapshot>, force_full: bool, now: Instant) -> serde_json::Result<String>`.
//! SSE snapshots omit unchanged `maps`, retaining their previous `maps_epoch`;
//! initial, gap recovery, and 10-second refresh snapshots contain all maps.
//!
//! File and IPC identities are defined in [`super::resource`].
//! [`snapshot::FdEndpoint::resource`] is `pub(super) resource: IpcIdentity`.
//!
//! [`snapshot::FdEndpoint`] also has `pub(super) kind: FdKind, access: FdAccess`.
//! [`snapshot::SocketEndpoint`] fields (all `pub(super)`):
//! `protocol: SocketProtocol, state: SocketState, local: Option<InetAddress>, remote: Option<InetAddress>, path: Option<String>, network_peer: bool, remote_hostname: Option<String>`.
//! Socket types are defined in [`super::socket_types`].
//!
//! Collector interfaces (all `pub(super)`, within `system`):
//! - [`activity::ActivityCollector`]: owns sensors and collection health; no service dependency.
//!   [`activity::ActivityCollector::new`]: `fn new() -> Self`;
//!   [`activity::ActivityCollector::poll`]: `fn poll(&mut self, snapshot: &SystemSnapshot)`;
//!   [`activity::ActivityCollector::drain`]: `fn drain(&mut self, now_ns: u64, snapshot: &SystemSnapshot) -> ActivityBatch`;
//!   [`activity::ActivityCollector::status`]: `fn status(&self) -> SystemMonitorStatus` (service sets `active`).
//! - [`activity::ActivityBatch`]: `files: Vec<FileActivity>, ipc: Vec<IpcActivity>, cpu: Vec<CpuActivity>`;
//!   all fields are `pub(super)`.
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
mod delivery;
pub(super) use delivery::SnapshotDelivery;
mod files;
mod ipc;
mod model;
mod resolver;
mod sched;
mod service;
mod snapshot;
mod status;
use service::monotonic_ns;
pub(super) use service::{SubscribeError, SystemMonitor, SystemMonitorEvent};
pub(super) use snapshot::SystemSnapshot;
use status::StatusLog;
