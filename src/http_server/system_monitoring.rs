//! System monitoring subscriptions and worker services.
//!
//! # Interface
//!
//! Re-export visibility: `pub(super)`. Names link to definitions; **Source** opens their implementation.
//!
//! | Definition | Kind |
//! | --- | --- |
//! | [`SubscribeError`] | `enum SubscribeError` |
//! | [`System`] | `struct System` |
//! | [`SystemEvent`] | `enum SystemEvent` |
//! | [`Topology`] | `struct Topology` |
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
