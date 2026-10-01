use super::procfs::{self, Stat};
use anyhow::Result;
use serde::Serialize;
use std::fs;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", content = "code", rename_all = "snake_case")]
pub(super) enum SchedulerPolicy {
    Other,
    Fifo,
    Rr,
    Batch,
    Idle,
    Deadline,
    Ext,
    Unknown(u32),
}
impl SchedulerPolicy {
    fn from_code(code: u32) -> Self {
        match code {
            0 => Self::Other,
            1 => Self::Fifo,
            2 => Self::Rr,
            3 => Self::Batch,
            5 => Self::Idle,
            6 => Self::Deadline,
            7 => Self::Ext,
            n => Self::Unknown(n),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(super) struct CpuRange {
    pub(super) start: u32,
    pub(super) end: u32,
}

fn affinity(text: &str) -> Option<Vec<CpuRange>> {
    text.split(',')
        .map(|item| {
            let (start, end) = item.split_once('-').unwrap_or((item, item));
            let start = start.parse().ok()?;
            let end = end.parse().ok()?;
            (start <= end).then_some(CpuRange { start, end })
        })
        .collect()
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct ThreadObservation {
    pub(super) tid: i32,
    pub(super) name: String,
    pub(super) state: String,
    pub(super) cpu: i32,
    pub(super) cpu_percent: Option<f64>,
    pub(super) priority: i64,
    pub(super) nice: i64,
    pub(super) scheduler: SchedulerPolicy,
    pub(super) affinity: Option<Vec<CpuRange>>,
    pub(super) voluntary_context_switches: Option<u64>,
    pub(super) nonvoluntary_context_switches: Option<u64>,
}

#[derive(Clone, Debug)]
pub(super) struct ThreadSample {
    pub(super) tid: i32,
    pub(super) name: String,
    pub(super) state: String,
    pub(super) cpu: i32,
    pub(super) priority: i64,
    pub(super) nice: i64,
    pub(super) scheduler: SchedulerPolicy,
    pub(super) affinity: Option<Vec<CpuRange>>,
    pub(super) voluntary_context_switches: Option<u64>,
    pub(super) nonvoluntary_context_switches: Option<u64>,
    pub(super) ticks: u64,
    pub(super) start_time: u64,
}

impl ThreadSample {
    pub(super) fn observation(&self) -> ThreadObservation {
        ThreadObservation {
            tid: self.tid,
            name: self.name.clone(),
            state: self.state.clone(),
            cpu: self.cpu,
            priority: self.priority,
            nice: self.nice,
            scheduler: self.scheduler,
            affinity: self.affinity.clone(),
            voluntary_context_switches: self.voluntary_context_switches,
            nonvoluntary_context_switches: self.nonvoluntary_context_switches,
            cpu_percent: None,
        }
    }
}

pub(super) fn tids(pid: i32) -> Result<Vec<i32>> {
    let mut tids: Vec<_> = fs::read_dir(format!("/proc/{pid}/task"))?
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse().ok())
        .collect();
    tids.sort_unstable();
    Ok(tids)
}

pub(super) fn read(pid: i32, tid: i32) -> Result<ThreadSample> {
    let path = format!("/proc/{pid}/task/{tid}");
    let s: Stat = procfs::read_stat(&format!("{path}/stat"))?;
    let f = procfs::fields(&format!("{path}/status")).unwrap_or_default();
    Ok(ThreadSample {
        tid,
        name: s.name,
        state: s.state,
        cpu: s.cpu,
        priority: s.priority,
        nice: s.nice,
        scheduler: SchedulerPolicy::from_code(s.policy),
        affinity: f.get("Cpus_allowed_list").and_then(|v| affinity(v)),
        voluntary_context_switches: procfs::field_u64(&f, "voluntary_ctxt_switches"),
        nonvoluntary_context_switches: procfs::field_u64(&f, "nonvoluntary_ctxt_switches"),
        ticks: s.ticks,
        start_time: s.start_time,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn affinity_ranges_and_unknown_scheduler_keep_values() {
        assert_eq!(
            affinity("0-3,8"),
            Some(vec![
                CpuRange { start: 0, end: 3 },
                CpuRange { start: 8, end: 8 }
            ])
        );
        assert_eq!(
            affinity("4294967295"),
            Some(vec![CpuRange {
                start: u32::MAX,
                end: u32::MAX
            }])
        );
        for text in ["", "3-1", "1,", "1-2-3", "4294967296", "x"] {
            assert!(affinity(text).is_none());
        }
        assert_eq!(
            serde_json::to_value(SchedulerPolicy::from_code(99)).unwrap(),
            serde_json::json!({"kind":"unknown","code":99})
        );
    }
}
