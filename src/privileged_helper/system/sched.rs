use anyhow::{Context, Result};

use libbpf_rs::{MapCore, MapFlags, ObjectBuilder};
/// Owns BPF resources and captures scheduler counters.
pub(super) struct CpuActivityCollector {
    _links: Vec<libbpf_rs::Link>,
    obj: libbpf_rs::Object,
}
#[cfg(test)]
use crate::ipc::activity::{ProcessKey, process_key};
impl CpuActivityCollector {
    pub(super) fn new() -> Result<Self> {
        let open = ObjectBuilder::default()
            .open_memory(include_bytes!(concat!(env!("OUT_DIR"), "/sched.bpf.o")))?;
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
        Ok(Self { _links: links, obj })
    }

    pub(super) fn collect(
        &mut self,
        now: u64,
        _snapshot: &super::snapshot::SystemSnapshot,
    ) -> Result<crate::ipc::activity::CpuCounters> {
        let current = self
            .obj
            .maps()
            .find(|map| map.name() == "cpu_current")
            .context("cpu_current map")?
            .lookup_percpu(&0u32.to_ne_bytes(), MapFlags::ANY)?
            .unwrap_or_default();
        let totals = self
            .obj
            .maps()
            .find(|map| map.name() == "cpu_totals")
            .context("cpu_totals map")?;

        let mut captured = Vec::new();
        for key in totals.keys().take(65_536) {
            if let Some(values) = totals.lookup_percpu(&key, MapFlags::ANY)? {
                let values = values
                    .into_iter()
                    .filter_map(|bytes| {
                        let runtime = u64::from_ne_bytes(bytes.get(..8)?.try_into().ok()?);
                        let switches = u64::from_ne_bytes(bytes.get(8..16)?.try_into().ok()?);
                        (runtime != 0 || switches != 0).then_some([runtime, switches])
                    })
                    .collect();
                captured.push((key, values));
            }
        }
        Ok(crate::ipc::activity::CpuCounters {
            now,
            current,
            totals: captured,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_process_key_with_kernel_layout_padding() {
        let mut bytes = [0u8; 16];
        bytes[..8].copy_from_slice(&9_876_543_210u64.to_ne_bytes());
        bytes[8..12].copy_from_slice(&4242u32.to_ne_bytes());
        assert_eq!(
            process_key(&bytes),
            Some(ProcessKey {
                start: 9_876_543_210,
                pid: 4242,
            })
        );
        assert!(process_key(&bytes[..11]).is_none());
    }
}
