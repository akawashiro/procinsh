//! Process observation and collector lifecycle.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`service`] | `pub(super)` | `mod service` |
//! | [`capture_sample`] | `pub(super)` | `fn capture_sample( id: `[`ProcessId`](crate::ipc::schema::ProcessId)`, ) -> `[`Result`](anyhow::Result)`<`[`ProcessSample`](crate::ipc::model::process::ProcessSample)`>` |
//! | [`ObservationSession`] | `pub(in crate::privileged_helper)` | `struct ObservationSession` |
//! | [`ProcessMonitor`] | `pub(in crate::privileged_helper)` | `struct ProcessMonitor` |
//! | [`create_process_monitor`] | `pub(in crate::privileged_helper)` | `fn create_process_monitor(interval: `[`Duration`](std::time::Duration)`) -> `[`ProcessMonitor`] |
//! | [`join_process_collectors`] | `pub(in crate::privileged_helper)` | `fn join_process_collectors( state: &`[`ProcessMonitor`]`, ) -> `[`Result`](anyhow::Result)`<()>` |
//! | [`observe_process_monitor`] | `pub(in crate::privileged_helper)` | `fn observe_process_monitor( monitor: &`[`Arc`](std::sync::Arc)`<`[`ProcessMonitor`]`>, id: `[`ProcessId`](crate::ipc::schema::ProcessId)`, permit: `[`ObservationPermit`](service::ObservationPermit)`, ) -> `[`Result`](anyhow::Result)`<`[`ObservationSession`]`>` |
//! | [`process_monitor_is_stopped`] | `pub(in crate::privileged_helper)` | `fn process_monitor_is_stopped(state: &`[`ProcessMonitor`]`) -> bool` |
//! | [`reserve_process_monitor`] | `pub(in crate::privileged_helper)` | `fn reserve_process_monitor( monitor: &`[`Arc`](std::sync::Arc)`<`[`ProcessMonitor`]`>, ) -> `[`Result`](anyhow::Result)`<`[`ObservationPermit`](service::ObservationPermit)`, `[`SubscribeError`](crate::privileged_helper::process::resources::SubscribeError)`>` |
//! | [`stop_process_monitor`] | `pub(in crate::privileged_helper)` | `fn stop_process_monitor(state: &`[`ProcessMonitor`]`)` |
//!
//! Sampling returns raw [`crate::ipc::model::process::ProcessSample`] counters with monotonic times.
//! [`ProcessMonitor`] owns acquisition sessions; main derives observations and histories.

mod sampling;
pub(super) mod service;
pub(super) use sampling::capture_sample;
pub(in crate::privileged_helper) use service::{
    ObservationSession, ProcessMonitor, create_process_monitor, join_process_collectors,
    observe_process_monitor, process_monitor_is_stopped, reserve_process_monitor,
    stop_process_monitor,
};

#[cfg(test)]
pub(in crate::privileged_helper) use service::observer_count_process_monitor;
