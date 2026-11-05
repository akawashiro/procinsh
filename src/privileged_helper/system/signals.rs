//! BPF signal_generate collection, bounded batching, and process identity checks.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind |
//! | --- | --- | --- |
//! | [`SignalCollector`] | `pub(super)` | `struct SignalCollector` |
//!
//! Drained observations use [`SignalSample`]; main matches them against captured process identities.

use super::snapshot::SystemSnapshot;
#[cfg(test)]
use crate::privileged_helper::process::{ProcessId, ticks_per_second};
use anyhow::{Context, Result};
use libbpf_rs::{MapCore, MapFlags, ObjectBuilder, RingBufferBuilder};
#[cfg(test)]
use std::collections::HashMap;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

const MAX_EVENTS: usize = 1024;

#[cfg(test)]
use crate::http_server::analysis::activity::resolve_signal;
use crate::ipc::activity::SignalSample;
use crate::privileged_helper::abi::decode_signal;
/// Owns the signal_generate hook and a bounded batch of process-level observations.
/// At most 1024 events are retained between service drains. Excess events contribute
/// to `lost`, independently of other sensors. Main matches process identities.
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
            if let Some(sample) = decode_signal(bytes) {
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

    pub(super) fn drain(&self, _snapshot: &SystemSnapshot) -> Vec<SignalSample> {
        self.queue.lock().unwrap().drain(..).collect()
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
        assert!(decode_signal(&[0; 39]).is_none());
        let mut bytes = [0u8; 40];
        bytes[0..8].copy_from_slice(&123456u64.to_ne_bytes());
        bytes[8..16].copy_from_slice(&1_000_000_000u64.to_ne_bytes());
        bytes[16..24].copy_from_slice(&2_000_000_000u64.to_ne_bytes());
        bytes[24..28].copy_from_slice(&42u32.to_ne_bytes());
        bytes[28..32].copy_from_slice(&43u32.to_ne_bytes());
        bytes[32..36].copy_from_slice(&libc::SIGUSR1.to_ne_bytes());
        let sample = decode_signal(&bytes).unwrap();
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
        let event = resolve_signal(sample, &processes).unwrap();
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
            resolve_signal(
                SignalSample {
                    signal: 0,
                    ..sample
                },
                &processes
            )
            .is_none()
        );
        processes.get_mut(&42).unwrap().start_time_ticks += 1;
        assert!(resolve_signal(sample, &processes).is_none());
        processes.get_mut(&42).unwrap().start_time_ticks -= 1;
        processes.get_mut(&43).unwrap().start_time_ticks += 1;
        assert!(resolve_signal(sample, &processes).is_none());
        processes.remove(&43);
        assert!(resolve_signal(sample, &processes).is_none());
    }
}
