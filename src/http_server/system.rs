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
//! | [`SystemSnapshot`] | `pub(super)` | `struct `[`SystemSnapshot`] |
//!
//! [`SystemMonitorEvent`] delivers structural snapshots and [`model::SystemActivity`]
//! sensor observations.

mod activity;
#[cfg(test)]
mod delivery;
#[cfg(test)]
pub(super) use delivery::SnapshotEncoder;
mod files;
mod hardware;
mod ipc;
mod ipoib;
mod model;
mod resolver;
mod sched;
mod service;
mod signals;
mod snapshot;
mod status;
use service::monotonic_ns;
pub(super) use service::{SubscribeError, Subscription, SystemMonitor, SystemMonitorEvent};
pub(super) use snapshot::SystemSnapshot;
use status::StatusLog;
