use crate::http_server::process::{
    self, ProcessId,
    procfs::{self, IoStats},
    threads::{self, ThreadObservation, ThreadSample},
};
use anyhow::Result;
use serde::Serialize;
use std::time::Instant;

#[derive(Clone, Debug, Default, Serialize)]
struct Rates {
    minor_faults: Option<f64>,
    major_faults: Option<f64>,
    voluntary_context_switches: Option<f64>,
    nonvoluntary_context_switches: Option<f64>,
    read_bytes: Option<f64>,
    write_bytes: Option<f64>,
}
/// Serializable counters and derived rates, without raw ticks or monotonic sampling state.
#[derive(Clone, Debug, Serialize)]
pub(in crate::http_server::process) struct ProcessObservation {
    pub(super) timestamp: u64,
    process_id: ProcessId,
    pub(super) cpu_percent: Option<f64>,
    pub(super) rss_bytes: u64,
    pub(super) vms_bytes: u64,
    minor_faults: u64,
    major_faults: u64,
    voluntary_context_switches: Option<u64>,
    nonvoluntary_context_switches: Option<u64>,
    io: Option<IoStats>,
    rates: Rates,
    pub(in crate::http_server::process) threads: Vec<ThreadObservation>,
    cpu: i32,
    nice: i64,
    priority: i64,
}

/// Raw process and thread counters with monotonic measurement time; no derived rates.
#[derive(Clone, Debug)]
pub(in crate::http_server::process) struct ProcessSample {
    pub(super) timestamp: u64,
    process_id: ProcessId,
    pub(super) rss_bytes: u64,
    pub(super) vms_bytes: u64,
    minor_faults: u64,
    major_faults: u64,
    voluntary_context_switches: Option<u64>,
    nonvoluntary_context_switches: Option<u64>,
    io: Option<IoStats>,
    pub(in crate::http_server::process) threads: Vec<ThreadSample>,
    cpu: i32,
    nice: i64,
    priority: i64,
    ticks: u64,
    measured_at: Instant,
}

pub(in crate::http_server::process) fn capture_sample(id: ProcessId) -> Result<ProcessSample> {
    process::check_identity(id)?;
    let stat = procfs::read_stat(&format!("/proc/{}/stat", id.pid))?;
    let now = Instant::now();
    let ts: Vec<_> = threads::tids(id.pid)?
        .into_iter()
        .filter_map(|tid| threads::read(id.pid, tid).ok())
        .collect();
    // /proc/PID/status contains only the leader's context switches: sum all live threads.
    let voluntary = ts
        .iter()
        .map(|t| t.voluntary_context_switches)
        .sum::<Option<u64>>();
    let nonvoluntary = ts
        .iter()
        .map(|t| t.nonvoluntary_context_switches)
        .sum::<Option<u64>>();
    let io = procfs::read_io(id.pid).ok();
    process::check_identity(id)?;
    Ok(ProcessSample {
        timestamp: process::timestamp_ms(),
        process_id: id,
        rss_bytes: stat.rss,
        vms_bytes: stat.vms,
        minor_faults: stat.minor_faults,
        major_faults: stat.major_faults,
        voluntary_context_switches: voluntary,
        nonvoluntary_context_switches: nonvoluntary,
        io,
        threads: ts,
        cpu: stat.cpu,
        nice: stat.nice,
        priority: stat.priority,
        ticks: stat.ticks,
        measured_at: now,
    })
}

/// Produces an initial observation with absent CPU percentages and rates.
pub(in crate::http_server::process) fn initial_observation(
    current: &ProcessSample,
) -> ProcessObservation {
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
pub(in crate::http_server::process) fn next_observation(
    previous: &ProcessSample,
    current: &ProcessSample,
) -> ProcessObservation {
    let mut result = initial_observation(current);
    if previous.process_id == current.process_id {
        let elapsed = current
            .measured_at
            .saturating_duration_since(previous.measured_at)
            .as_secs_f64();
        if elapsed > 0.0 {
            let rate = |a: u64, b: u64| a.checked_sub(b).map(|d| d as f64 / elapsed);
            result.cpu_percent =
                rate(current.ticks, previous.ticks).map(|r| r / procfs::ticks_per_second() * 100.0);
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
                        rate(t.ticks, old.ticks).map(|r| r / procfs::ticks_per_second() * 100.0);
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn sample() -> ProcessSample {
        let id = process::identity(std::process::id() as i32).unwrap();
        let mut sample = capture_sample(id).unwrap();
        sample.threads.truncate(1);
        sample.ticks = 100;
        sample.minor_faults = 10;
        sample.major_faults = 20;
        sample.io = Some(IoStats {
            read_bytes: 100,
            write_bytes: 200,
        });
        let thread = &mut sample.threads[0];
        thread.ticks = 100;
        thread.voluntary_context_switches = Some(10);
        thread.nonvoluntary_context_switches = Some(20);
        sample
    }

    #[test]
    fn initial_observation_has_no_rates_or_sampling_state() {
        let current = sample();
        let value = serde_json::to_value(initial_observation(&current)).unwrap();
        assert_eq!(value["rss_bytes"], current.rss_bytes);
        assert!(value["cpu_percent"].is_null());
        assert!(
            value["rates"]
                .as_object()
                .unwrap()
                .values()
                .all(serde_json::Value::is_null)
        );
        assert!(value.get("ticks").is_none());
        assert!(value.get("measured_at").is_none());
        assert!(value["threads"][0]["cpu_percent"].is_null());
        assert!(value["threads"][0].get("ticks").is_none());
        assert!(value["threads"][0].get("start_time").is_none());
    }

    #[test]
    fn next_observation_derives_all_rates_without_mutating_samples() {
        let previous = sample();
        let mut current = previous.clone();
        current.measured_at += Duration::from_secs(2);
        current.ticks += 40;
        current.minor_faults += 6;
        current.major_faults += 4;
        current.io.as_mut().unwrap().read_bytes += 200;
        current.io.as_mut().unwrap().write_bytes += 400;
        current.threads[0].ticks += 20;
        current.threads[0].voluntary_context_switches = Some(18);
        current.threads[0].nonvoluntary_context_switches = Some(26);
        let observed = next_observation(&previous, &current);
        assert_eq!(
            observed.cpu_percent,
            Some(20.0 / procfs::ticks_per_second() * 100.0)
        );
        assert_eq!(
            observed.threads[0].cpu_percent,
            Some(10.0 / procfs::ticks_per_second() * 100.0)
        );
        assert_eq!(observed.rates.minor_faults, Some(3.0));
        assert_eq!(observed.rates.major_faults, Some(2.0));
        assert_eq!(observed.rates.voluntary_context_switches, Some(4.0));
        assert_eq!(observed.rates.nonvoluntary_context_switches, Some(3.0));
        assert_eq!(observed.rates.read_bytes, Some(100.0));
        assert_eq!(observed.rates.write_bytes, Some(200.0));
        assert_eq!(previous.ticks, 100);
        assert_eq!(current.ticks, 140);
    }

    #[test]
    fn thread_churn_counter_resets_missing_io_and_zero_elapsed() {
        let previous = sample();
        let mut current = previous.clone();
        assert!(next_observation(&previous, &current).cpu_percent.is_none());
        current.measured_at += Duration::from_secs(1);
        current.threads[0].start_time += 1; // Same tid, different thread.
        current.threads[0].ticks = 1;
        current.threads[0].voluntary_context_switches = Some(1);
        current.io = None;
        current.ticks = 1;
        let observed = next_observation(&previous, &current);
        assert!(observed.cpu_percent.is_none());
        assert!(observed.threads[0].cpu_percent.is_none());
        assert_eq!(observed.rates.voluntary_context_switches, Some(0.0));
        assert!(observed.rates.read_bytes.is_none());
        current.threads[0].start_time = previous.threads[0].start_time;
        assert!(
            next_observation(&previous, &current)
                .rates
                .voluntary_context_switches
                .is_none()
        );
        current.threads[0].voluntary_context_switches = None;
        assert!(
            next_observation(&previous, &current)
                .rates
                .voluntary_context_switches
                .is_none()
        );
        current.process_id.start_time_ticks += 1;
        assert!(
            next_observation(&previous, &current)
                .rates
                .minor_faults
                .is_none()
        );
    }
}
