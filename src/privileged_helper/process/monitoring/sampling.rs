//! Acquire raw process/thread counters with monotonic timestamps.
//!
//! # Interface
//! [`capture_sample`] (`pub(super)`): `fn capture_sample(id: ProcessId) -> anyhow::Result<ProcessSample>`.
//! Shared observation values and calculations come from [`crate::ipc::model::process`].
#[cfg(test)]
use super::super::procfs::IoStats;
#[cfg(test)]
pub(in crate::privileged_helper::process) use crate::http_server::model::process::{
    initial_observation, next_observation,
};
pub(in crate::privileged_helper::process) use crate::ipc::model::process::ProcessSample;
use crate::privileged_helper::process::{self, ProcessId, procfs, threads};
use anyhow::Result;

pub(in crate::privileged_helper::process) fn capture_sample(
    id: ProcessId,
) -> Result<ProcessSample> {
    process::validate_process_identity(id)?;
    let stat = procfs::read_stat(&format!("/proc/{}/stat", id.pid))?;
    let now = crate::ipc::model::process::monotonic_ns();
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
    process::validate_process_identity(id)?;
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

#[cfg(test)]
mod tests {
    use super::*;

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
        current.measured_at += 2_000_000_000;
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
        current.measured_at += 1_000_000_000;
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
