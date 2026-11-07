use crate::ipc::activity::IpcEvent;
use crate::privileged_helper::bpf_records::decode_ipc;
use anyhow::{Context, Result};
use libbpf_rs::{MapCore, MapFlags, ObjectBuilder, RingBufferBuilder};
use std::sync::{Arc, Mutex};
/// Owns BPF resources and captures bounded raw IPC observations.
pub(super) struct IpcActivityCollector {
    ring: libbpf_rs::RingBuffer<'static>,
    _links: Vec<libbpf_rs::Link>,
    obj: libbpf_rs::Object,
    queue: Arc<Mutex<Vec<IpcEvent>>>,
    drops: Arc<std::sync::atomic::AtomicU64>,
}

/// Kernels before the anonymous pipe split, such as 6.8, name the pipe
/// handlers `pipe_read`/`pipe_write` with the same arguments. Without vmlinux
/// BTF the targets stay unchanged so loading reports the BTF requirement.
fn pipe_attach_target(program: &str) -> Option<&'static str> {
    let fallback = match program {
        "anon_pipe_read" => "pipe_read",
        "anon_pipe_write" => "pipe_write",
        _ => return None,
    };
    let btf = libbpf_rs::btf::Btf::from_vmlinux().ok()?;
    btf.type_by_name::<libbpf_rs::btf::types::Func>(program)
        .is_none()
        .then_some(fallback)
}

impl IpcActivityCollector {
    pub(super) fn new() -> Result<Self> {
        let mut open = ObjectBuilder::default()
            .open_memory(include_bytes!(concat!(env!("OUT_DIR"), "/ipc.bpf.o")))?;
        for mut prog in open.progs_mut() {
            let Some(target) = prog.name().to_str().and_then(pipe_attach_target) else {
                continue;
            };
            prog.set_attach_target(0, Some(target.into()))
                .with_context(|| format!("retarget pipe tracing to {target}"))?;
        }
        let obj = open
            .load()
            .context("CAP_BPF / CAP_PERFMON and compatible BTF required")?;
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
            if let Some(e) = decode_ipc(bytes) {
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

    pub(super) fn poll(
        &mut self,
        _snapshot: &crate::ipc::model::system::SystemSnapshot,
    ) -> Result<()> {
        let consumed = self.ring.consume_raw_n(8192);
        if consumed < 0 {
            return Err(std::io::Error::from_raw_os_error(-consumed).into());
        }
        Ok(())
    }
    pub(super) fn drain(&mut self) -> Vec<IpcEvent> {
        self.queue.lock().unwrap().drain(..).collect()
    }
    pub(super) fn unresolved(&self) -> u64 {
        0
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_bpf_event_without_unaligned_reads() {
        assert!(decode_ipc(&[0; 55]).is_none());
        let mut bytes = [0u8; 56];
        bytes[8..16].copy_from_slice(&1234u64.to_ne_bytes());
        bytes[32..40].copy_from_slice(&256u64.to_ne_bytes());
        bytes[40..44].copy_from_slice(&42u32.to_ne_bytes());
        bytes[48..52].copy_from_slice(&1u32.to_ne_bytes());
        let e = decode_ipc(&bytes).unwrap();
        assert_eq!(e.start, 1234);
        assert_eq!(e.bytes, 256);
        assert_eq!(e.pid, 42);
        assert!(e.write);
        assert!(!e.worker);
    }
}
