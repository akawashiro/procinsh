//! System monitoring subscriptions and worker services.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind |
//! | --- | --- | --- |
//! | [`SubscribeError`] | `pub(super)` | `enum `[`SubscribeError`] |
//! | [`System`] | `pub(super)` | `struct `[`System`] |
//! | [`SystemEvent`] | `pub(super)` | `enum `[`SystemEvent`] |
//! | [`Topology`] | `pub(super)` | `struct `[`Topology`] |
mod activity;
mod files;
mod resolver;
mod service;
mod status;
mod topology;
use service::monotonic_ns;
pub(super) use service::{SubscribeError, System, SystemEvent};
use status::StatusLog;
pub(super) use topology::Topology;
