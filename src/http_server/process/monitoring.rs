//! Process observation and collector lifecycle.
//!
//! # Interface
//!
//! Names link to definitions; **Source** opens their implementation.
//!
//! | Definition | Re-export visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`Monitoring`] | `pub(in crate::http_server)` | `struct Monitoring` |
//! | [`observation`] | `pub(super)` | `fn observation(id: ProcessId, previous: Option<&ProcessObservation>) -> anyhow::Result<ProcessObservation>` |
//!
//! Types: [`ProcessId`](super::identity::ProcessId), [`ProcessObservation`].
mod history;
mod service;
pub(in crate::http_server) use service::Monitoring;
use service::ProcessObservation;
pub(super) use service::observation;
