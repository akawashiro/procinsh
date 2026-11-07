//! Acquire thread identities, scheduler fields and counters.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`collect_thread_ids`] | `pub(super)` | `fn collect_thread_ids(pid: i32) -> Result<Vec<i32>>` |
//! | [`read_thread_sample`] | `pub(super)` | `fn read_thread_sample(pid: i32, tid: i32) -> Result<ThreadSample>` |
//! | [`SchedulerPolicy`] | `pub(super)` | `enum SchedulerPolicy` |
//! | [`CpuRange`] | `pub(super)` | `struct CpuRange` |
//! | [`parse_cpu_affinity`] | `pub(super)` | `fn parse_cpu_affinity(text: &str) -> Option<Vec<CpuRange>>` |
//! | [`ThreadObservation`] | `pub(super)` | `struct ThreadObservation` |
//! | [`ThreadSample`] | `pub(super)` | `struct ThreadSample` |

use super::procfs::{self, Stat};
pub(super) use crate::ipc::model::threads::*;
use anyhow::{Result, ensure};
use std::fs;

pub(super) fn collect_thread_ids(pid: i32) -> Result<Vec<i32>> {
    let mut tids: Vec<_> = fs::read_dir(format!("/proc/{pid}/task"))?
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse().ok())
        .take(1025)
        .collect();
    ensure!(
        tids.len() <= 1024,
        "Process exceeds 1024-thread inspection limit"
    );
    tids.sort_unstable();
    Ok(tids)
}

pub(super) fn read_thread_sample(pid: i32, tid: i32) -> Result<ThreadSample> {
    let path = format!("/proc/{pid}/task/{tid}");
    let s: Stat = procfs::read_process_stat(&format!("{path}/stat"))?;
    let f = procfs::read_procfs_fields(&format!("{path}/status")).unwrap_or_default();
    Ok(ThreadSample {
        tid,
        name: s.name,
        state: s.state,
        cpu: s.cpu,
        priority: s.priority,
        nice: s.nice,
        scheduler: SchedulerPolicy::from_code(s.policy),
        affinity: f
            .get("Cpus_allowed_list")
            .and_then(|v| parse_cpu_affinity(v)),
        voluntary_context_switches: procfs::parse_procfs_u64_field(&f, "voluntary_ctxt_switches"),
        nonvoluntary_context_switches: procfs::parse_procfs_u64_field(
            &f,
            "nonvoluntary_ctxt_switches",
        ),
        ticks: s.ticks,
        start_time: s.start_time,
    })
}
