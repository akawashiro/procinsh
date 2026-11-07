//! Raw process counters and calculations performed after IPC receipt.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`ticks_per_second`] | `pub` | `fn ticks_per_second() -> f64` |
//! | [`IoStats`] | `pub` | `struct IoStats` |
//! | [`Rates`] | `pub` | `struct Rates` |
//! | [`ProcessObservation`] | `pub` | `struct ProcessObservation` |
//! | [`ProcessSample`] | `pub` | `struct ProcessSample` |
//! | [`monotonic_ns`] | `pub` | `fn monotonic_ns() -> u64` |

use crate::ipc::{
    model::threads::{ThreadObservation, ThreadSample},
    schema::ProcessId,
};

pub fn ticks_per_second() -> f64 {
    unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as f64
}

#[derive(Clone, Debug, Default)]
pub struct IoStats {
    pub read_bytes: u64,
    pub write_bytes: u64,
}
#[derive(Clone, Debug, Default)]
pub struct Rates {
    pub minor_faults: Option<f64>,
    pub major_faults: Option<f64>,
    pub voluntary_context_switches: Option<f64>,
    pub nonvoluntary_context_switches: Option<f64>,
    pub read_bytes: Option<f64>,
    pub write_bytes: Option<f64>,
}
/// Counters and derived rates produced by main.
#[derive(Clone, Debug)]
pub struct ProcessObservation {
    pub timestamp: u64,
    pub process_id: ProcessId,
    pub cpu_percent: Option<f64>,
    pub rss_bytes: u64,
    pub vms_bytes: u64,
    pub minor_faults: u64,
    pub major_faults: u64,
    pub voluntary_context_switches: Option<u64>,
    pub nonvoluntary_context_switches: Option<u64>,
    pub io: Option<IoStats>,
    pub rates: Rates,
    pub threads: Vec<ThreadObservation>,
    pub cpu: i32,
    pub nice: i64,
    pub priority: i64,
}

/// Raw process and thread counters with monotonic measurement time; no derived rates.
#[derive(Clone, Debug)]
pub struct ProcessSample {
    pub timestamp: u64,
    pub process_id: ProcessId,
    pub rss_bytes: u64,
    pub vms_bytes: u64,
    pub minor_faults: u64,
    pub major_faults: u64,
    pub voluntary_context_switches: Option<u64>,
    pub nonvoluntary_context_switches: Option<u64>,
    pub io: Option<IoStats>,
    pub threads: Vec<ThreadSample>,
    pub cpu: i32,
    pub nice: i64,
    pub priority: i64,
    pub ticks: u64,
    pub measured_at: u64,
}

/// Monotonic acquisition time in nanoseconds.
pub fn monotonic_ns() -> u64 {
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
