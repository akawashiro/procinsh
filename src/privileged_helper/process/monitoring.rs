//! Process observation and collector lifecycle.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`ObservationSession`] | `pub(in crate::privileged_helper)` | `struct ObservationSession` |
//! | [`ProcessMonitor`] | `pub(in crate::privileged_helper)` | `struct `[`ProcessMonitor`] |
//! | [`capture_sample`] | `pub(super)` | `fn capture_sample(id: ProcessId) -> anyhow::Result<ProcessSample>` |
//!
//! Sampling returns raw [`crate::ipc::model::process::ProcessSample`] counters with monotonic times.
//! [`ProcessMonitor`] owns acquisition sessions; main derives observations and histories.

mod sampling;
mod service;
pub(super) use sampling::capture_sample;
pub(in crate::privileged_helper) use service::{ObservationSession, ProcessMonitor};
