//! System monitoring subscriptions, snapshot delivery and worker services.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind |
//! | --- | --- | --- |
//! | [`SubscribeError`] | `pub(super)` | `enum `[`SubscribeError`] |
//! | [`Subscription`] | `pub(super)` | `struct Subscription` |
//! | [`SystemMonitor`] | `pub(super)` | `struct `[`SystemMonitor`] |
//! | [`SystemMonitorEvent`] | `pub(super)` | `enum `[`SystemMonitorEvent`] |
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
