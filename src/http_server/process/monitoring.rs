//! Process observation and collector lifecycle.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`Monitoring`] | `pub(in crate::http_server)` | `struct `[`Monitoring`] |
//! | [`capture_sample`] | `pub(super)` | `fn capture_sample(id: ProcessId) -> anyhow::Result<ProcessSample>` |
//! | [`initial_observation`] | `pub(super)` | `fn initial_observation(current: &ProcessSample) -> ProcessObservation` |
//!
//! Sampling uses raw [`sampling::ProcessSample`] counters to produce serializable
//! [`sampling::ProcessObservation`] values. [`Monitoring`] owns observation sessions.

mod history;
mod sampling;
mod service;
use sampling::ProcessObservation;
pub(super) use sampling::{capture_sample, initial_observation};
pub(in crate::http_server) use service::Monitoring;
