//! Fork boundary, privilege management, and bounded monitor IPC.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`run`] | `pub(crate)` | `fn run(listen: &[`[`SocketAddr`](std::net::SocketAddr)`]) -> anyhow::Result<()>` |
//! | [`MonitorClient`] | `pub(in crate::http_server)` | `struct MonitorClient` |
//! | [`MonitorRequest`] | `pub(in crate::http_server)` | `enum MonitorRequest` |
//! | [`MonitorResponse`] | `pub(in crate::http_server)` | `enum MonitorResponse` |
//! | [`MonitorEvent`] | `pub(in crate::http_server)` | `enum MonitorEvent` |
//! | [`ProcessResource`] | `pub(in crate::http_server)` | `enum ProcessResource` |
//! | [`ObservationSession`] | `pub(in crate::http_server)` | `struct ObservationSession` |
//!
//! Requests use [`ProcessId`](super::process::ProcessId), responses carry
//! [`SystemSnapshot`](super::system::SystemSnapshot) or structured JSON values.
//! Descriptors never cross the inherited socketpair. See [`protocol`] for limits.

mod client;
mod lifecycle;
mod privileges;
mod protocol;
mod service;
mod transport;

pub(super) use client::{MonitorClient, ObservationSession};
pub(crate) use lifecycle::run;
pub(super) use protocol::{MonitorEvent, MonitorRequest, MonitorResponse, ProcessResource};

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(super) use service::MonitorState;
