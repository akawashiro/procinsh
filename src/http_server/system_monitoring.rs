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
mod activity;
mod files;
mod resolver;
mod service;
mod status;
mod system_snapshot;
use service::monotonic_ns;
pub(super) use service::{SubscribeError, System, SystemEvent};
use status::StatusLog;
pub(super) use system_snapshot::SystemSnapshot;
