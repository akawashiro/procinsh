//! Process observation and collector lifecycle.
//!
//! # Interface
//!
//! | Re-export visibility | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`Monitoring`] | `pub(in crate::http_server)` | `struct `[`Monitoring`] |
//! | [`observation`] | `pub(super)` | `fn observation(id: `[`ProcessId`](super::identity::ProcessId)`, previous: `[`Option`]`<&`[`ProcessObservation`]`>) -> `[`anyhow::Result`]`<`[`ProcessObservation`]`>` |
//!
mod history;
mod service;
pub(in crate::http_server) use service::Monitoring;
use service::ProcessObservation;
pub(super) use service::observation;
