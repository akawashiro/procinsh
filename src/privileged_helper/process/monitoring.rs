//! Process observation and collector lifecycle.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`SubscribeError`] | `pub(in crate::privileged_helper)` | `enum SubscribeError` |
//! | [`collect_process_observation`] | `pub(in crate::privileged_helper)` | `fn collect_process_observation( id: `[`ProcessId`](crate::ipc::schema::ProcessId)`, ) -> `[`Result`](anyhow::Result)`<`[`crate::ipc::acquisition::ObservationData`]`>` |
//! | [`collect_thread_observation`] | `pub(in crate::privileged_helper)` | `fn collect_thread_observation( id: `[`ProcessId`](crate::ipc::schema::ProcessId)`, ) -> `[`Result`](anyhow::Result)`<`[`crate::ipc::acquisition::ObservationData`]`>` |
//! | [`service`] | `pub(super)` | `mod service` |
//! | [`ObservationSession`] | `pub(in crate::privileged_helper)` | `struct ObservationSession` |
//! | [`ProcessMonitor`] | `pub(in crate::privileged_helper)` | `struct ProcessMonitor` |
//! | [`create_process_monitor`] | `pub(in crate::privileged_helper)` | `fn create_process_monitor(interval: `[`Duration`](std::time::Duration)`) -> `[`ProcessMonitor`] |
//! | [`join_process_collectors`] | `pub(in crate::privileged_helper)` | `fn join_process_collectors( state: &`[`ProcessMonitor`]`, ) -> `[`Result`](anyhow::Result)`<()>` |
//! | [`observe_process_monitor`] | `pub(in crate::privileged_helper)` | `fn observe_process_monitor( monitor: &`[`Arc`](std::sync::Arc)`<`[`ProcessMonitor`]`>, id: `[`ProcessId`](crate::ipc::schema::ProcessId)`, permit: `[`ObservationPermit`](service::ObservationPermit)`, ) -> `[`Result`](anyhow::Result)`<`[`ObservationSession`]`>` |
//! | [`process_monitor_is_stopped`] | `pub(in crate::privileged_helper)` | `fn process_monitor_is_stopped(state: &`[`ProcessMonitor`]`) -> bool` |
//! | [`reserve_process_monitor`] | `pub(in crate::privileged_helper)` | `fn reserve_process_monitor( monitor: &`[`Arc`](std::sync::Arc)`<`[`ProcessMonitor`]`>, ) -> `[`Result`](anyhow::Result)`<`[`ObservationPermit`](service::ObservationPermit)`, `[`SubscribeError`]`>` |
//! | [`stop_process_monitor`] | `pub(in crate::privileged_helper)` | `fn stop_process_monitor(state: &`[`ProcessMonitor`]`)` |
//!
//! Sampling returns raw [`crate::ipc::model::process::ProcessSample`] counters with monotonic times.
//! [`ProcessMonitor`] owns acquisition sessions; main derives observations and histories.

mod sampling;
pub(super) mod service;
pub(in crate::privileged_helper) use sampling::{
    collect_process_observation, collect_thread_observation,
};
pub(in crate::privileged_helper) use service::{
    ObservationSession, ProcessMonitor, SubscribeError, create_process_monitor,
    join_process_collectors, observe_process_monitor, process_monitor_is_stopped,
    reserve_process_monitor, stop_process_monitor,
};

#[cfg(test)]
pub(in crate::privileged_helper) use service::observer_count_process_monitor;
