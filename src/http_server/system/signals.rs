//! BPF signal_generate collection, bounded batching, and process identity checks.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind |
//! | --- | --- | --- |
//! | [`SignalCollector`] | `pub(super)` | `struct SignalCollector` |
//!
//! Drained observations use [`SignalEvent`] and are matched against [`SystemSnapshot`].

use super::{model::SignalEvent, snapshot::SystemSnapshot};
use crate::http_server::process::{ProcessId, ticks_per_second};
use anyhow::{Context, Result};
use libbpf_rs::{MapCore, MapFlags, ObjectBuilder, RingBufferBuilder};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

const MAX_EVENTS: usize = 1024;

#[derive(Clone, Copy, Debug)]
struct SignalSample {
    timestamp_ns: u64,
    src_start: u64,
    dst_start: u64,
    src_pid: u32,
    dst_pid: u32,
    signal: i32,
}

fn decode(bytes: &[u8]) -> Option<SignalSample> {
    if bytes.len() != 40 {
        return None;
    }
    let u64at = |i| u64::from_ne_bytes(bytes[i..i + 8].try_into().unwrap());
    let u32at = |i| u32::from_ne_bytes(bytes[i..i + 4].try_into().unwrap());
    Some(SignalSample {
        timestamp_ns: u64at(0),
        src_start: u64at(8),
        dst_start: u64at(16),
        src_pid: u32at(24),
        dst_pid: u32at(28),
        signal: u32at(32) as i32,
    })
}

fn resolve(sample: SignalSample, processes: &HashMap<i32, ProcessId>) -> Option<SignalEvent> {
    let source_id = *processes.get(&(sample.src_pid as i32))?;
    let destination_id = *processes.get(&(sample.dst_pid as i32))?;
    let ticks = |start: u64| (start as u128 * ticks_per_second() as u128 / 1_000_000_000) as u64;
    if sample.signal <= 0
        || sample.signal > 64
        || ticks(sample.src_start) != source_id.start_time_ticks
        || ticks(sample.dst_start) != destination_id.start_time_ticks
    {
        return None;
    }
    Some(SignalEvent {
        timestamp_ns: sample.timestamp_ns,
        src_pid: sample.src_pid,
        dst_pid: sample.dst_pid,
        signal: sample.signal,
        source_id,
        destination_id,
    })
}

/// Owns the signal_generate hook and a bounded batch of process-level observations.
/// At most 1024 events are retained between service drains. Excess events contribute
/// to `lost`, independently of other sensors. Unrepresented or reused PIDs are ignored.
pub(super) struct SignalCollector {
    ring: libbpf_rs::RingBuffer<'static>,
    _links: Vec<libbpf_rs::Link>,
    obj: libbpf_rs::Object,
    queue: Arc<Mutex<Vec<SignalSample>>>,
    drops: Arc<AtomicU64>,
}

impl SignalCollector {
    pub(super) fn new() -> Result<Self> {
        let obj = ObjectBuilder::default()
            .open_memory(include_bytes!(concat!(env!("OUT_DIR"), "/signals.bpf.o")))?
            .load()
            .context("signal collection requires CAP_BPF / CAP_PERFMON and compatible BTF")?;
        let mut links = Vec::new();
        for prog in obj.progs_mut() {
            links.push(prog.attach().context("attach signal_generate")?);
        }
        let queue = Arc::new(Mutex::new(Vec::new()));
        let q = queue.clone();
        let drops = Arc::new(AtomicU64::new(0));
        let d = drops.clone();
        let map = obj
            .maps()
            .find(|m| m.name() == "events")
            .context("signal events map")?;
        let mut builder = RingBufferBuilder::new();
        builder.add(&map, move |bytes| {
            if let Some(sample) = decode(bytes) {
                let mut queue = q.lock().unwrap();
                if queue.len() < MAX_EVENTS {
                    queue.push(sample);
                } else {
                    d.fetch_add(1, Ordering::Relaxed);
                }
            }
            0
        })?;
        Ok(Self {
            ring: builder.build()?,
            _links: links,
            obj,
            queue,
            drops,
        })
    }

    pub(super) fn poll(&self) -> Result<()> {
        let consumed = self.ring.consume_raw_n(8192);
        if consumed < 0 {
            return Err(std::io::Error::from_raw_os_error(-consumed).into());
        }
        Ok(())
    }

    pub(super) fn drain(&self, snapshot: &SystemSnapshot) -> Vec<SignalEvent> {
        let processes = snapshot
            .processes
            .iter()
            .map(|n| (n.identity.pid, n.identity))
            .collect();
        self.queue
            .lock()
            .unwrap()
            .drain(..)
            .filter_map(|sample| resolve(sample, &processes))
            .collect()
    }

    pub(super) fn lost(&self) -> u64 {
        self.obj
            .maps()
            .find(|m| m.name() == "lost")
            .and_then(|m| m.lookup(&0u32.to_ne_bytes(), MapFlags::ANY).ok().flatten())
            .and_then(|v| v.try_into().ok())
            .map(u64::from_ne_bytes)
            .unwrap_or(0)
            .saturating_add(self.drops.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_sample_decoding_and_pid_reuse() {
        assert!(decode(&[0; 39]).is_none());
        let mut bytes = [0u8; 40];
        bytes[0..8].copy_from_slice(&123456u64.to_ne_bytes());
        bytes[8..16].copy_from_slice(&1_000_000_000u64.to_ne_bytes());
        bytes[16..24].copy_from_slice(&2_000_000_000u64.to_ne_bytes());
        bytes[24..28].copy_from_slice(&42u32.to_ne_bytes());
        bytes[28..32].copy_from_slice(&43u32.to_ne_bytes());
        bytes[32..36].copy_from_slice(&libc::SIGUSR1.to_ne_bytes());
        let sample = decode(&bytes).unwrap();
        let mut processes = HashMap::from([
            (
                42,
                ProcessId {
                    pid: 42,
                    start_time_ticks: ticks_per_second() as u64,
                },
            ),
            (
                43,
                ProcessId {
                    pid: 43,
                    start_time_ticks: ticks_per_second() as u64 * 2,
                },
            ),
        ]);
        let event = resolve(sample, &processes).unwrap();
        assert_eq!(
            (
                event.timestamp_ns,
                event.src_pid,
                event.dst_pid,
                event.signal
            ),
            (123456, 42, 43, libc::SIGUSR1)
        );
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["source_id"]["pid"], 42);
        assert_eq!(json["destination_id"]["pid"], 43);
        assert!(
            resolve(
                SignalSample {
                    signal: 0,
                    ..sample
                },
                &processes
            )
            .is_none()
        );
        processes.get_mut(&42).unwrap().start_time_ticks += 1;
        assert!(resolve(sample, &processes).is_none());
        processes.get_mut(&42).unwrap().start_time_ticks -= 1;
        processes.get_mut(&43).unwrap().start_time_ticks += 1;
        assert!(resolve(sample, &processes).is_none());
        processes.remove(&43);
        assert!(resolve(sample, &processes).is_none());
    }
}
