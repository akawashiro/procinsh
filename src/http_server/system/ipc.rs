//! Collects process-attributed pipe and socket activity using an Aya sensor.
//!
//! # Interface
//!
//! - [`IpcActivityCollector`] (`pub(super) struct`): owns, polls and drains IPC observations.
use super::model::IpcActivity;
use crate::http_server::resource::{DeviceId, IpcIdentity, IpcKind};
use anyhow::{Context, Result};
use aya::maps::{Array, MapData, RingBuf};
use std::collections::HashMap;
#[derive(Clone, Copy)]
struct Event {
    start: u64,
    inode: u64,
    device: u64,
    bytes: u64,
    pid: u32,
    kind: u32,
    write: bool,
    worker: bool,
}

fn event(bytes: &[u8]) -> Option<Event> {
    if bytes.len() != 56 {
        return None;
    }
    let u64at = |i| u64::from_ne_bytes(bytes[i..i + 8].try_into().unwrap());
    let u32at = |i| u32::from_ne_bytes(bytes[i..i + 4].try_into().unwrap());
    Some(Event {
        start: u64at(8),
        inode: u64at(16),
        device: u64at(24),
        bytes: u64at(32),
        pid: u32at(40),
        kind: u32at(44),
        write: u32at(48) != 0,
        worker: u32at(52) != 0,
    })
}
/// Owns BPF resources and collects IPC activity attributed to observed processes.
pub(super) struct IpcActivityCollector {
    ring: RingBuf<MapData>,
    lost: Array<MapData, u64>,
    _obj: aya::Ebpf,
    unresolved: u64,
    pending: HashMap<(crate::http_server::process::ProcessId, IpcIdentity, bool), (u64, u64)>,
}

impl IpcActivityCollector {
    pub(super) fn new() -> Result<Self> {
        let mut obj = super::bpf::load("ipc")?;
        let ring = RingBuf::try_from(obj.take_map("events").context("IPC events map")?)?;
        let lost = Array::try_from(obj.take_map("lost").context("IPC lost map")?)?;
        Ok(Self {
            ring,
            lost,
            _obj: obj,
            unresolved: 0,
            pending: HashMap::new(),
        })
    }

    pub(super) fn lost(&self) -> u64 {
        self.lost.get(&0, 0).unwrap_or(0)
    }

    pub(super) fn poll(&mut self, snapshot: &super::snapshot::SystemSnapshot) -> Result<()> {
        let processes: HashMap<_, _> = snapshot
            .processes
            .iter()
            .map(|n| (n.identity.pid, n))
            .collect();
        for _ in 0..8192 {
            let Some(item) = self.ring.next() else {
                break;
            };
            let Some(e) = event(&item) else {
                continue;
            };
            let Some(n) = processes.get(&(e.pid as i32)) else {
                self.unresolved += 1;
                continue;
            };
            let ticks = ((e.start as u128
                * crate::http_server::process::ticks_per_second() as u128)
                / 1_000_000_000) as u64;
            if e.worker || ticks != n.identity.start_time_ticks {
                self.unresolved += 1;
                continue;
            }
            let resource = IpcIdentity {
                kind: if e.kind == 1 {
                    IpcKind::Pipe
                } else {
                    IpcKind::Socket
                },
                device: DeviceId::from_kernel(e.device),
                inode: e.inode,
            };
            if self.pending.len() >= 8192 {
                self.unresolved += 1;
                continue;
            }
            let value = self
                .pending
                .entry((n.identity, resource, e.write))
                .or_insert((0u64, 0u64));
            value.0 += e.bytes;
            value.1 += 1;
        }
        Ok(())
    }

    pub(super) fn drain(&mut self) -> Vec<IpcActivity> {
        self.pending
            .drain()
            .map(
                |((process_id, resource, write), (bytes, count))| IpcActivity {
                    process_id,
                    resource,
                    write,
                    bytes,
                    count,
                },
            )
            .collect()
    }

    pub(super) fn unresolved(&self) -> u64 {
        self.unresolved
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_bpf_event_without_unaligned_reads() {
        assert!(event(&[0; 55]).is_none());
        let mut bytes = [0u8; 56];
        bytes[8..16].copy_from_slice(&1234u64.to_ne_bytes());
        bytes[32..40].copy_from_slice(&256u64.to_ne_bytes());
        bytes[40..44].copy_from_slice(&42u32.to_ne_bytes());
        bytes[48..52].copy_from_slice(&1u32.to_ne_bytes());
        let e = event(&bytes).unwrap();
        assert_eq!(e.start, 1234);
        assert_eq!(e.bytes, 256);
        assert_eq!(e.pid, 42);
        assert!(e.write);
        assert!(!e.worker);
    }
}
