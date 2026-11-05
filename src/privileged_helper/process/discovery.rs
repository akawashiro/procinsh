// Visibility is scoped to the consumers of the parent process façade.
use super::{ProcessId, procfs};
use anyhow::Result;
use std::{collections::HashMap, fs};

pub use crate::ipc::acquisition::ProcessSummary;

pub(super) fn summary(stat: &procfs::Stat, users: &HashMap<u32, String>) -> ProcessSummary {
    let base = format!("/proc/{}", stat.pid);
    let (uid, euid) = procfs::fields(&format!("{base}/status"))
        .ok()
        .map(|f| parse_uids(f.get("Uid").map(String::as_str).unwrap_or("")))
        .unwrap_or((None, None));
    ProcessSummary {
        identity: ProcessId {
            pid: stat.pid,
            start_time_ticks: stat.start_time,
        },
        parent_pid: stat.parent_pid,
        name: stat.name.clone(),
        executable: fs::read_link(format!("{base}/exe"))
            .ok()
            .map(|p| p.to_string_lossy().into_owned()),
        command_line: fs::read(format!("{base}/cmdline")).ok().map(|b| {
            b.split(|c| *c == 0)
                .filter(|s| !s.is_empty())
                .map(|s| String::from_utf8_lossy(s).into_owned())
                .collect()
        }),
        uid,
        username: uid.and_then(|u| users.get(&u).cloned()),
        euid,
        effective_username: euid.and_then(|u| users.get(&u).cloned()),
        state: stat.state.clone(),
        started_at: started_at(stat.start_time),
        cpu_percent: None,
        rss_bytes: stat.rss,
        thread_count: stat.thread_count,
        raw_ticks: stat.ticks,
        raw_measured_ns: crate::ipc::model::process::monotonic_ns(),
    }
}

fn started_at(ticks: u64) -> Option<u64> {
    // Avoid reading /proc/stat for every process in the discovery loop.
    static BOOT_TIME: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
    let boot = *BOOT_TIME.get_or_init(|| {
        fs::read_to_string("/proc/stat")
            .ok()?
            .lines()
            .find_map(|line| line.strip_prefix("btime ")?.trim().parse::<u64>().ok())
    });
    start_timestamp(boot?, ticks, procfs::ticks_per_second())
}

fn start_timestamp(boot_seconds: u64, ticks: u64, ticks_per_second: f64) -> Option<u64> {
    boot_seconds
        .checked_mul(1000)?
        .checked_add((ticks as f64 / ticks_per_second * 1000.0) as u64)
}

pub(super) fn users() -> HashMap<u32, String> {
    {
        fs::read_to_string("/etc/passwd")
            .unwrap_or_default()
            .lines()
            .filter_map(|line| {
                let p: Vec<_> = line.split(':').collect();
                Some((p.get(2)?.parse().ok()?, p.first()?.to_string()))
            })
            .collect()
    }
}

/// Scans protected procfs for process summaries and raw CPU counters.
pub(in crate::privileged_helper) fn collect_processes() -> Result<Vec<ProcessSummary>> {
    let users = users();
    let mut result = Vec::new();
    for entry in fs::read_dir("/proc")?.flatten() {
        let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<i32>().ok())
        else {
            continue;
        };
        let Ok(stat) = procfs::read_stat(&format!("/proc/{pid}/stat")) else {
            continue;
        };
        result.push(summary(&stat, &users));
    }
    Ok(result)
}

fn parse_uids(value: &str) -> (Option<u32>, Option<u32>) {
    let mut values = value.split_whitespace();
    (
        values.next().and_then(|v| v.parse().ok()),
        values.next().and_then(|v| v.parse().ok()),
    )
}

#[cfg(test)]
mod uid_tests {
    use super::*;

    #[test]
    fn start_time_converts_boot_relative_ticks_to_unix_milliseconds() {
        assert_eq!(
            start_timestamp(1_700_000_000, 341232, 100.0),
            Some(1_700_003_412_320)
        );
        assert_eq!(start_timestamp(u64::MAX, 0, 100.0), None);
    }

    #[test]
    fn real_and_effective_users() {
        assert_eq!(parse_uids("1000 0 0 0"), (Some(1000), Some(0)));
        assert_eq!(parse_uids("42 42 42 42"), (Some(42), Some(42)));
        assert_eq!(parse_uids(""), (None, None));
    }
}
