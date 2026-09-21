use super::{ProcessId, procfs};
use anyhow::Result;
use serde::Serialize;
use std::{collections::HashMap, fs, time::Instant};

#[derive(Clone, Debug, Serialize)]
pub struct ProcessSummary {
    pub identity: ProcessId,
    #[serde(skip)]
    pub parent_pid: i32,
    pub name: String,
    pub executable: Option<String>,
    pub command_line: Option<Vec<String>>,
    pub uid: Option<u32>,
    pub username: Option<String>,
    pub euid: Option<u32>,
    pub effective_username: Option<String>,
    pub state: String,
    pub cpu_percent: Option<f64>,
    pub rss_bytes: u64,
    pub thread_count: u32,
}

pub fn summary(stat: &procfs::Stat, users: &HashMap<u32, String>) -> ProcessSummary {
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
        cpu_percent: None,
        rss_bytes: stat.rss,
        thread_count: stat.thread_count,
    }
}

pub fn users() -> HashMap<u32, String> {
    fs::read_to_string("/etc/passwd")
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let p: Vec<_> = line.split(':').collect();
            Some((p.get(2)?.parse().ok()?, p.first()?.to_string()))
        })
        .collect()
}

#[derive(Default)]
pub struct Discovery {
    previous: HashMap<ProcessId, (u64, Instant)>,
}
impl Discovery {
    pub fn collect(&mut self) -> Result<Vec<ProcessSummary>> {
        let users = users();
        let now = Instant::now();
        let mut next = HashMap::new();
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
            let mut item = summary(&stat, &users);
            if let Some((ticks, time)) = self.previous.get(&item.identity) {
                let elapsed = now.duration_since(*time).as_secs_f64();
                if elapsed > 0.0 {
                    item.cpu_percent = Some(
                        stat.ticks.saturating_sub(*ticks) as f64
                            / procfs::ticks_per_second()
                            / elapsed
                            * 100.0,
                    );
                }
            }
            next.insert(item.identity, (stat.ticks, now));
            result.push(item);
        }
        self.previous = next;
        result.sort_by(|a, b| {
            b.cpu_percent
                .unwrap_or(0.0)
                .total_cmp(&a.cpu_percent.unwrap_or(0.0))
                .then(a.identity.pid.cmp(&b.identity.pid))
        });
        Ok(result)
    }
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
    fn real_and_effective_users() {
        assert_eq!(parse_uids("1000 0 0 0"), (Some(1000), Some(0)));
        assert_eq!(parse_uids("42 42 42 42"), (Some(42), Some(42)));
        assert_eq!(parse_uids(""), (None, None));
    }
}
