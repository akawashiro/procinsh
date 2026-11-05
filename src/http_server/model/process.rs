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
//! | [`initial_observation`] | `pub` | `fn initial_observation(current: &ProcessSample) -> ProcessObservation` |
//! | [`next_observation`] | `pub` | `fn next_observation(previous: &ProcessSample, current: &ProcessSample) -> ProcessObservation` |
//! | [`monotonic_ns`] | `pub` | `fn monotonic_ns() -> u64` |

pub use crate::ipc::model::process::*;
use crate::{
    http_server::model::threads::{ThreadObservation, ThreadSample},
    ipc::schema::ProcessId,
};

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::http_server::model::process::IoStats")]
struct IoStatsJson {
    pub read_bytes: u64,
    pub write_bytes: u64,
}
impl serde::Serialize for crate::http_server::model::process::IoStats {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        IoStatsJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::http_server::model::process::IoStats {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        IoStatsJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::http_server::model::process::Rates")]
struct RatesJson {
    pub minor_faults: Option<f64>,
    pub major_faults: Option<f64>,
    pub voluntary_context_switches: Option<f64>,
    pub nonvoluntary_context_switches: Option<f64>,
    pub read_bytes: Option<f64>,
    pub write_bytes: Option<f64>,
}
impl serde::Serialize for crate::http_server::model::process::Rates {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        RatesJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::http_server::model::process::Rates {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        RatesJson::deserialize(d)
    }
}

/// Serializable counters and derived rates, without raw ticks or monotonic sampling state.
#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::http_server::model::process::ProcessObservation")]
struct ProcessObservationJson {
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
impl serde::Serialize for crate::http_server::model::process::ProcessObservation {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        ProcessObservationJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::http_server::model::process::ProcessObservation {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        ProcessObservationJson::deserialize(d)
    }
}

/// Raw process and thread counters with monotonic measurement time; no derived rates.
#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::http_server::model::process::ProcessSample")]
struct ProcessSampleJson {
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
impl serde::Serialize for crate::http_server::model::process::ProcessSample {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        ProcessSampleJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::http_server::model::process::ProcessSample {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        ProcessSampleJson::deserialize(d)
    }
}

/// Produces an initial observation with absent CPU percentages and rates.
pub fn initial_observation(current: &ProcessSample) -> ProcessObservation {
    ProcessObservation {
        timestamp: current.timestamp,
        process_id: current.process_id,
        rss_bytes: current.rss_bytes,
        vms_bytes: current.vms_bytes,
        minor_faults: current.minor_faults,
        major_faults: current.major_faults,
        voluntary_context_switches: current.voluntary_context_switches,
        nonvoluntary_context_switches: current.nonvoluntary_context_switches,
        io: current.io.clone(),
        cpu: current.cpu,
        nice: current.nice,
        priority: current.priority,
        cpu_percent: None,
        rates: Rates::default(),
        threads: current
            .threads
            .iter()
            .map(ThreadSample::observation)
            .collect(),
    }
}

/// Derives rates from monotonic elapsed time for the same process identity.
/// Thread rates match both TID and start time; missing or decreasing counters yield absent rates.
pub fn next_observation(previous: &ProcessSample, current: &ProcessSample) -> ProcessObservation {
    let mut result = initial_observation(current);
    if previous.process_id == current.process_id {
        let elapsed =
            current.measured_at.saturating_sub(previous.measured_at) as f64 / 1_000_000_000.0;
        if elapsed > 0.0 {
            let rate = |a: u64, b: u64| a.checked_sub(b).map(|d| d as f64 / elapsed);
            result.cpu_percent =
                rate(current.ticks, previous.ticks).map(|r| r / ticks_per_second() * 100.0);
            result.rates.minor_faults = rate(current.minor_faults, previous.minor_faults);
            result.rates.major_faults = rate(current.major_faults, previous.major_faults);
            // Match thread identities so churn cannot make process context-switch rates negative.
            let mut v = Some(0u64);
            let mut n = Some(0u64);
            for (t, observed) in current.threads.iter().zip(&mut result.threads) {
                if let Some(old) = previous
                    .threads
                    .iter()
                    .find(|old| old.tid == t.tid && old.start_time == t.start_time)
                {
                    observed.cpu_percent =
                        rate(t.ticks, old.ticks).map(|r| r / ticks_per_second() * 100.0);
                    v = v
                        .zip(
                            t.voluntary_context_switches
                                .zip(old.voluntary_context_switches)
                                .and_then(|(a, b)| a.checked_sub(b)),
                        )
                        .map(|(a, b)| a + b);
                    n = n
                        .zip(
                            t.nonvoluntary_context_switches
                                .zip(old.nonvoluntary_context_switches)
                                .and_then(|(a, b)| a.checked_sub(b)),
                        )
                        .map(|(a, b)| a + b);
                }
            }
            result.rates.voluntary_context_switches = v.map(|v| v as f64 / elapsed);
            result.rates.nonvoluntary_context_switches = n.map(|v| v as f64 / elapsed);
            if let (Some(a), Some(b)) = (&current.io, &previous.io) {
                result.rates.read_bytes = rate(a.read_bytes, b.read_bytes);
                result.rates.write_bytes = rate(a.write_bytes, b.write_bytes);
            }
        }
    }
    result
}
