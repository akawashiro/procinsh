use super::procfs::{self, Stat};
use anyhow::Result;
use serde::Serialize;
use std::fs;

#[derive(Clone, Debug, Serialize)]
pub struct ThreadObservation {
    pub tid: i32,
    pub name: String,
    pub state: String,
    pub cpu: i32,
    pub cpu_percent: Option<f64>,
    pub priority: i64,
    pub nice: i64,
    pub scheduler: String,
    pub affinity: Option<String>,
    pub voluntary_context_switches: Option<u64>,
    pub nonvoluntary_context_switches: Option<u64>,
    #[serde(skip)]
    pub ticks: u64,
    #[serde(skip)]
    pub start_time: u64,
}

pub fn tids(pid: i32) -> Result<Vec<i32>> {
    let mut tids: Vec<_> = fs::read_dir(format!("/proc/{pid}/task"))?
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse().ok())
        .collect();
    tids.sort_unstable();
    Ok(tids)
}

pub fn read(pid: i32, tid: i32) -> Result<ThreadObservation> {
    let path = format!("/proc/{pid}/task/{tid}");
    let s: Stat = procfs::read_stat(&format!("{path}/stat"))?;
    let f = procfs::fields(&format!("{path}/status")).unwrap_or_default();
    Ok(ThreadObservation {
        tid,
        name: s.name,
        state: s.state,
        cpu: s.cpu,
        cpu_percent: None,
        priority: s.priority,
        nice: s.nice,
        scheduler: match s.policy {
            0 => "OTHER",
            1 => "FIFO",
            2 => "RR",
            3 => "BATCH",
            5 => "IDLE",
            6 => "DEADLINE",
            7 => "EXT",
            _ => "UNKNOWN",
        }
        .into(),
        affinity: f.get("Cpus_allowed_list").cloned(),
        voluntary_context_switches: procfs::field_u64(&f, "voluntary_ctxt_switches"),
        nonvoluntary_context_switches: procfs::field_u64(&f, "nonvoluntary_ctxt_switches"),
        ticks: s.ticks,
        start_time: s.start_time,
    })
}
