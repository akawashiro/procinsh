use super::model::IpcActivity;
use crate::http_server::resource::{DeviceId, IpcIdentity, IpcKind};
use anyhow::{Context, Result};
use libbpf_rs::{MapCore, MapFlags, ObjectBuilder, RingBufferBuilder};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
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
pub(super) struct Ipc {
    ring: libbpf_rs::RingBuffer<'static>,
    _links: Vec<libbpf_rs::Link>,
    obj: libbpf_rs::Object,
    queue: Arc<Mutex<Vec<Event>>>,
    drops: Arc<std::sync::atomic::AtomicU64>,
    unresolved: u64,
    pending: HashMap<(crate::http_server::process::ProcessId, IpcIdentity, bool), (u64, u64)>,
}

impl Ipc {
    pub(super) fn new(namespace: super::pidns::PidNamespace) -> Result<Self> {
        let open = ObjectBuilder::default()
            .open_memory(include_bytes!(concat!(env!("OUT_DIR"), "/ipc.bpf.o")))?;
        let obj = open
            .load()
            .context("CAP_BPF / CAP_PERFMON and compatible BTF required")?;
        namespace.configure(&obj)?;
        let mut links = Vec::new();
        for prog in obj.progs_mut() {
            links.push(
                prog.attach()
                    .with_context(|| format!("attach {:?}", prog.name()))?,
            );
        }
        let queue = Arc::new(Mutex::new(Vec::new()));
        let q = queue.clone();
        let drops = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let d = drops.clone();
        let map = obj
            .maps()
            .find(|m| m.name() == "events")
            .context("events map")?;
        let mut builder = RingBufferBuilder::new();
        builder.add(&map, move |bytes| {
            if let Some(e) = event(bytes) {
                let mut q = q.lock().unwrap();
                if q.len() < 65536 {
                    q.push(e);
                } else {
                    d.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
            0
        })?;
        let ring = builder.build()?;
        Ok(Self {
            ring,
            _links: links,
            obj,
            queue,
            drops,
            unresolved: 0,
            pending: HashMap::new(),
        })
    }

    pub(super) fn lost(&self) -> u64 {
        self.obj
            .maps()
            .find(|m| m.name() == "lost")
            .and_then(|m| m.lookup(&0u32.to_ne_bytes(), MapFlags::ANY).ok().flatten())
            .and_then(|v| v.try_into().ok())
            .map(u64::from_ne_bytes)
            .unwrap_or(0)
            + self.drops.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(super) fn poll(&mut self, snapshot: &super::system_snapshot::SystemSnapshot) -> Result<()> {
        let consumed = self.ring.consume_raw_n(8192);
        if consumed < 0 {
            return Err(std::io::Error::from_raw_os_error(-consumed).into());
        }
        let processes: HashMap<_, _> = snapshot
            .processes
            .iter()
            .map(|n| (n.identity.pid, n))
            .collect();
        for e in self.queue.lock().unwrap().drain(..) {
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
