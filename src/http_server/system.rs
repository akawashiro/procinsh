//! System monitoring subscriptions, snapshot delivery and worker services.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind |
//! | --- | --- | --- |
//! | [`SubscribeError`] | `pub(super)` | `enum `[`SubscribeError`] |
//! | [`SystemMonitor`] | `pub(super)` | `struct `[`SystemMonitor`] |
//! | [`SystemMonitorEvent`] | `pub(super)` | `enum `[`SystemMonitorEvent`] |
//! | [`SystemSnapshot`] | `pub(super)` | `struct `[`SystemSnapshot`] |
//! | [`SnapshotEncoder`] | `pub(super)` | `struct `[`SnapshotEncoder`] |
//!
//! [`SystemMonitorEvent`] delivers structural snapshots and [`model::SystemActivity`]
//! sensor observations. [`SnapshotEncoder`] encodes snapshots for each SSE connection.

mod activity;
mod delivery;
pub(super) use delivery::SnapshotEncoder;
mod files;
mod ipc;
mod model;
mod resolver;
mod sched;
mod service;
mod signals;
mod snapshot;
mod status;
use service::monotonic_ns;
pub(super) use service::{SubscribeError, SystemMonitor, SystemMonitorEvent};
pub(super) use snapshot::SystemSnapshot;
use status::StatusLog;
