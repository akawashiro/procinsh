use crate::{
    inspect::{frames::StackFrame, registers::Register},
    process::ProcessId,
};
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};

pub const LIMIT: usize = 16 * 1024 * 1024;
#[derive(Clone, Debug, Serialize)]
pub struct ThreadSample {
    pub tid: i32,
    pub start_time_ticks: u64,
    pub sampled_at_mono_ns: String,
    pub sample_age_ms: u64,
    #[serde(serialize_with = "crate::process::maps::hex")]
    pub ip: u64,
    pub cpu: u32,
    pub registers: Vec<Register>,
    pub call_stack: Vec<StackFrame>,
    pub unwind_stop: String,
    pub error: Option<String>,
    pub quality: String,
    #[serde(skip)]
    pub time: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct HistoryPoint {
    pub tid: i32,
    pub start_time_ticks: u64,
    pub sampled_at_mono_ns: String,
    #[serde(serialize_with = "crate::process::maps::hex")]
    pub ip: u64,
    pub cpu: u32,
    #[serde(skip)]
    time: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct Samples {
    pub process_id: ProcessId,
    pub status: String,
    pub collected_at_mono_ns: String,
    pub configured_hz: u32,
    pub lost_total: u64,
    pub malformed_total: u64,
    pub history_dropped_total: u64,
    pub thread_limit_reached: bool,
    pub throttled: bool,
    pub monitored_threads: usize,
    pub warnings: Vec<String>,
    pub threads: Vec<ThreadSample>,
    pub history: VecDeque<HistoryPoint>,
}
pub struct Store {
    pub data: Samples,
    latest: BTreeMap<i32, ThreadSample>,
    sizes: BTreeMap<i32, usize>,
    bytes: usize,
    limit: usize,
}
impl Store {
    pub fn new(id: ProcessId, hz: u32) -> Self {
        Self {
            data: Samples {
                process_id: id,
                status: "active".into(),
                collected_at_mono_ns: super::mono_ns().to_string(),
                configured_hz: hz,
                lost_total: 0,
                malformed_total: 0,
                history_dropped_total: 0,
                thread_limit_reached: false,
                monitored_threads: 0,
                throttled: false,
                warnings: Vec::new(),
                threads: Vec::new(),
                history: VecDeque::new(),
            },
            latest: BTreeMap::new(),
            sizes: BTreeMap::new(),
            bytes: 0,
            limit: LIMIT,
        }
    }
    pub fn push(&mut self, sample: ThreadSample) {
        let size = serde_json::to_vec(&sample).map_or(0, |b| b.len()) + 512;
        // Bound even pathological symbol names/maps. Leave IP and timestamp useful.
        let mut sample = sample;
        if size > 64 * 1024 {
            sample.registers.clear();
            sample.call_stack.clear();
            sample.quality = "ip_only".into();
            sample.error = Some("Sample detail exceeded 64 KiB limit".into());
        }
        let size = size.min(64 * 1024) + 512;
        if let Some(old) = self.sizes.insert(sample.tid, size) {
            self.bytes -= old;
        }
        self.bytes += size;
        self.latest.insert(sample.tid, sample);
        self.trim(super::mono_ns());
    }
    pub fn record(&mut self, raw: &super::decode::Sample, start: u64) {
        self.data.history.push_back(HistoryPoint {
            tid: raw.tid,
            start_time_ticks: start,
            sampled_at_mono_ns: raw.time.to_string(),
            ip: raw.ip,
            cpu: raw.cpu,
            time: raw.time,
        });
        self.trim(super::mono_ns());
    }
    fn trim(&mut self, now: u64) {
        while self
            .data
            .history
            .front()
            .is_some_and(|h| now.saturating_sub(h.time) > 60_000_000_000)
            || (!self.data.history.is_empty()
                && self.bytes + self.data.history.len() * 256 > self.limit)
        {
            self.data.history.pop_front();
            self.data.history_dropped_total = self.data.history_dropped_total.saturating_add(1);
        }
    }
    pub fn retain(&mut self, identities: &BTreeMap<i32, u64>) {
        self.latest
            .retain(|tid, s| identities.get(tid) == Some(&s.start_time_ticks));
        self.sizes.retain(|tid, _| self.latest.contains_key(tid));
        self.bytes = self.sizes.values().sum();
    }
    pub fn view(&mut self) -> Samples {
        let now = super::mono_ns();
        self.trim(now);
        // Per-TID rings may arrive out of timestamp order. Expire the entire
        // lightweight history at publication, not just the front of the deque.
        let before = self.data.history.len();
        self.data
            .history
            .retain(|h| now.saturating_sub(h.time) <= 60_000_000_000);
        self.data.history_dropped_total = self
            .data
            .history_dropped_total
            .saturating_add((before - self.data.history.len()) as u64);
        let mut result = self.data.clone();
        result.collected_at_mono_ns = now.to_string();
        result.threads = self
            .latest
            .values()
            .cloned()
            .map(|mut s| {
                s.sample_age_ms = now.saturating_sub(s.time) / 1_000_000;
                s
            })
            .collect();
        result
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retention_and_identity() {
        let mut store = Store::new(
            ProcessId {
                pid: 1,
                start_time_ticks: 1,
            },
            49,
        );
        store.limit = 2048;
        for i in 0..100 {
            store.push(ThreadSample {
                tid: 1,
                start_time_ticks: 10,
                sampled_at_mono_ns: i.to_string(),
                time: super::super::mono_ns(),
                sample_age_ms: 0,
                ip: 1,
                cpu: 0,
                registers: vec![],
                call_stack: vec![],
                unwind_stop: String::new(),
                error: None,
                quality: "ip_only".into(),
            });
        }
        for _ in 0..100 {
            store.record(
                &super::super::decode::Sample {
                    ip: 1,
                    tid: 1,
                    time: super::super::mono_ns(),
                    cpu: 0,
                    regs: vec![],
                    frames: vec![],
                    abi: None,
                },
                10,
            );
        }
        assert!(store.data.history_dropped_total > 0);
        assert_eq!(store.view().threads.len(), 1);
        store.retain(&BTreeMap::from([(1, 11)]));
        assert!(store.view().threads.is_empty());
    }
    #[test]
    fn expires_out_of_order_history_and_serializes_times_without_precision_loss() {
        let mut store = Store::new(
            ProcessId {
                pid: 1,
                start_time_ticks: 1,
            },
            49,
        );
        let now = super::super::mono_ns();
        for (tid, time) in [(1, now), (2, now.saturating_sub(61_000_000_000))] {
            store.record(
                &super::super::decode::Sample {
                    ip: 0x1234,
                    tid,
                    time,
                    cpu: 0,
                    regs: vec![],
                    frames: vec![],
                    abi: None,
                },
                1,
            );
        }
        let data = store.view();
        assert_eq!(data.history.len(), 1);
        let json = serde_json::to_value(data).unwrap();
        assert!(json["collected_at_mono_ns"].is_string());
        assert!(json["history"][0]["sampled_at_mono_ns"].is_string());
        assert_eq!(json["history"][0]["ip"], "0x0000000000001234");
    }
}
