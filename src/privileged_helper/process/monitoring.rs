//! Process observation and collector lifecycle.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`ObservationSession`] | `pub(in crate::privileged_helper)` | `struct ObservationSession` |
//! | [`ProcessMonitor`] | `pub(in crate::privileged_helper)` | `struct `[`ProcessMonitor`] |
//! | [`capture_sample`] | `pub(super)` | `fn capture_sample(id: ProcessId) -> anyhow::Result<ProcessSample>` |
//! | [`initial_observation`] | `pub(super)` | `fn initial_observation(current: &ProcessSample) -> ProcessObservation` |
//!
//! Sampling uses raw [`sampling::ProcessSample`] counters to produce serializable
//! [`sampling::ProcessObservation`] values. [`ProcessMonitor`] owns observation sessions.

mod history;
mod sampling;
mod service;
use sampling::ProcessObservation;
pub(super) use sampling::{capture_sample, initial_observation};
pub(in crate::privileged_helper) use service::{ObservationSession, ProcessMonitor};
