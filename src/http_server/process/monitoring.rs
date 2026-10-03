//! Process observation and collector lifecycle.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`Monitoring`] | `pub(in crate::http_server)` | `struct `[`Monitoring`] |
//! | [`ProcessSample`](sampling::ProcessSample) | `pub(in crate::http_server::process)` | Raw counters, thread samples and measurement time; no derived rates. |
//! | [`ProcessObservation`] | `pub(in crate::http_server::process)` | Serializable observation without sampling state. |
//! | [`capture_sample`] | `pub(super)` | `fn capture_sample(id: ProcessId) -> anyhow::Result<ProcessSample>` |
//! | [`initial_observation`] | `pub(super)` | `fn initial_observation(current: &ProcessSample) -> ProcessObservation` |
//! | [`next_observation`](sampling::next_observation) | `pub(in crate::http_server::process)` | `fn next_observation(previous: &ProcessSample, current: &ProcessSample) -> ProcessObservation` |
//! | [`service::capture_target`] | `pub(in crate::http_server::process)` | `fn capture_target(id: ProcessId) -> anyhow::Result<(Target, ProcessSample)>` |
//!
//! [`sampling::ProcessSample`] stores `timestamp: u64, process_id: ProcessId, ticks: u64,
//! measured_at: Instant, rss_bytes: u64, vms_bytes: u64, minor_faults: u64, major_faults: u64,
//! voluntary_context_switches: Option<u64>, nonvoluntary_context_switches: Option<u64>,
//! io: Option<IoStats>, threads: Vec<ThreadSample>, cpu: i32, nice: i64, priority: i64`.
//! [`ProcessObservation`] keeps the public counters and metadata, with `cpu_percent: Option<f64>,
//! rates: Rates, threads: Vec<ThreadObservation>`; it has no `ticks` or `measured_at`.
//!
//! [`service::Target`] adds `live_samples: Vec<super::snapshot::ThreadSample>, sampling_error: Option<String>`
//! to each SSE state; samples retain their own monotonic age and Unix `sampled_at`.
mod history;
mod sampling;
mod service;
use sampling::ProcessObservation;
pub(super) use sampling::{capture_sample, initial_observation};
pub(in crate::http_server) use service::Monitoring;
