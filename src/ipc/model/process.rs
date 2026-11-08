//! Raw process counters and calculations performed after IPC receipt.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`IoStats`] | `pub(crate)` | `struct IoStats` |
//! | [`Rates`] | `pub(crate)` | `struct Rates` |
//! | [`ProcessObservation`] | `pub(crate)` | `struct ProcessObservation` |
//! | [`ProcessSample`] | `pub(crate)` | `struct ProcessSample` |
//! | [`ticks_per_second`] | `pub(crate)` | `fn ticks_per_second() -> f64` |
//! | [`monotonic_ns`] | `pub(crate)` | `fn monotonic_ns() -> u64` |

use crate::ipc::{
    model::threads::{ThreadObservation, ThreadSample},
    schema::ProcessId,
};

pub(crate) fn ticks_per_second() -> f64 {
    unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as f64
}

#[derive(Clone, Debug, Default)]
pub(crate) struct IoStats {
    pub(crate) read_bytes: u64,
    pub(crate) write_bytes: u64,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct Rates {
    pub(crate) minor_faults: Option<f64>,
    pub(crate) major_faults: Option<f64>,
    pub(crate) voluntary_context_switches: Option<f64>,
    pub(crate) nonvoluntary_context_switches: Option<f64>,
    pub(crate) read_bytes: Option<f64>,
    pub(crate) write_bytes: Option<f64>,
}
/// Counters and derived rates produced by main.
#[derive(Clone, Debug)]
pub(crate) struct ProcessObservation {
    pub(crate) timestamp: u64,
    pub(crate) process_id: ProcessId,
    pub(crate) cpu_percent: Option<f64>,
    pub(crate) rss_bytes: u64,
    pub(crate) vms_bytes: u64,
    pub(crate) minor_faults: u64,
    pub(crate) major_faults: u64,
    pub(crate) voluntary_context_switches: Option<u64>,
    pub(crate) nonvoluntary_context_switches: Option<u64>,
    pub(crate) io: Option<IoStats>,
    pub(crate) rates: Rates,
    pub(crate) threads: Vec<ThreadObservation>,
    pub(crate) cpu: i32,
    pub(crate) nice: i64,
    pub(crate) priority: i64,
}

/// Raw process and thread counters with monotonic measurement time; no derived rates.
#[derive(Clone, Debug)]
pub(crate) struct ProcessSample {
    pub(crate) timestamp: u64,
    pub(crate) process_id: ProcessId,
    pub(crate) rss_bytes: u64,
    pub(crate) vms_bytes: u64,
    pub(crate) minor_faults: u64,
    pub(crate) major_faults: u64,
    pub(crate) voluntary_context_switches: Option<u64>,
    pub(crate) nonvoluntary_context_switches: Option<u64>,
    pub(crate) io: Option<IoStats>,
    pub(crate) threads: Vec<ThreadSample>,
    pub(crate) cpu: i32,
    pub(crate) nice: i64,
    pub(crate) priority: i64,
    pub(crate) ticks: u64,
    pub(crate) measured_at: u64,
}

/// Monotonic acquisition time in nanoseconds.
pub(crate) fn monotonic_ns() -> u64 {
    let mut time: libc::timespec = unsafe { std::mem::zeroed() };
    unsafe { libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time) };
    time.tv_sec as u64 * 1_000_000_000 + time.tv_nsec as u64
}

crate::ipc::codec::wire_struct!(IoStats {
    read_bytes,
    write_bytes
});

crate::ipc::codec::wire_struct!(Rates {
    minor_faults,
    major_faults,
    voluntary_context_switches,
    nonvoluntary_context_switches,
    read_bytes,
    write_bytes
});

crate::ipc::codec::wire_struct!(ProcessObservation {
    timestamp,
    process_id,
    cpu_percent,
    rss_bytes,
    vms_bytes,
    minor_faults,
    major_faults,
    voluntary_context_switches,
    nonvoluntary_context_switches,
    io,
    rates,
    threads,
    cpu,
    nice,
    priority
});

crate::ipc::codec::wire_struct!(ProcessSample {
    timestamp,
    process_id,
    rss_bytes,
    vms_bytes,
    minor_faults,
    major_faults,
    voluntary_context_switches,
    nonvoluntary_context_switches,
    io,
    threads,
    cpu,
    nice,
    priority,
    ticks,
    measured_at
});
