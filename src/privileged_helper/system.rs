//! System monitoring subscriptions, snapshot delivery and worker services.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`SubscribeError`] | `pub(super)` | `enum SubscribeError` |
//! | [`Subscription`] | `pub(super)` | `struct Subscription` |
//! | [`SystemMonitor`] | `pub(super)` | `struct SystemMonitor` |
//! | [`SystemMonitorEvent`] | `pub(super)` | `enum SystemMonitorEvent` |
//! | [`join_system_monitor_workers`] | `pub(in crate::privileged_helper)` | `fn join_system_monitor_workers( state: &`[`SystemMonitor`]`, ) -> `[`anyhow::Result`]`<()>` |
//! | [`snapshot_system_monitor`] | `pub(in crate::privileged_helper)` | `fn snapshot_system_monitor( state: &`[`SystemMonitor`]`, ) -> `[`Arc`](std::sync::Arc)`<`[`SystemSnapshot`](crate::ipc::model::system::SystemSnapshot)`>` |
//! | [`stop_system_monitor`] | `pub(in crate::privileged_helper)` | `fn stop_system_monitor(state: &`[`SystemMonitor`]`)` |
//! | [`subscribe_system_monitor`] | `pub(in crate::privileged_helper)` | `fn subscribe_system_monitor( state: &`[`Arc`](std::sync::Arc)`<`[`SystemMonitor`]`>, ) -> `[`Result`]`<`[`Subscription`]`, `[`SubscribeError`]`>` |
//!
//! [`SystemMonitorEvent`] delivers structural snapshots and [`crate::ipc::acquisition::SystemActivity`]
//! sensor observations.

mod activity;
#[cfg(test)]
mod delivery;
#[cfg(test)]
pub(super) use delivery::SnapshotEncoder;
mod files;
mod hardware;
#[cfg(not(test))]
mod hardware_capture;
#[cfg(test)]
use crate::http_server::analysis::hardware::acquisition;
mod ipc;
mod ipoib;
mod model;
mod rdma;
mod resolver;
mod sched;
mod service;
mod signals;
mod snapshot;
mod status;
use service::monotonic_ns;
pub(super) use service::{SubscribeError, Subscription, SystemMonitor, SystemMonitorEvent};
use status::StatusLog;

pub(in crate::privileged_helper) use service::{
    join_system_monitor_workers, snapshot_system_monitor, stop_system_monitor,
    subscribe_system_monitor,
};

#[cfg(test)]
pub(in crate::privileged_helper) use delivery::encode_snapshot_encoder;

#[cfg(test)]
pub(in crate::privileged_helper) use service::{active_system_monitor, stopped_system_monitor};
