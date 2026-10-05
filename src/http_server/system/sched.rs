use super::model::CpuActivity;
use anyhow::{Context, Result};
use libbpf_rs::{MapCore, MapFlags, ObjectBuilder};
use std::collections::HashMap;
/// Owns BPF resources and derives CPU activity from scheduler counters.
pub(super) struct CpuActivityCollector {
    _links: Vec<libbpf_rs::Link>,
    obj: libbpf_rs::Object,
    previous_cpu: HashMap<ProcessKey, (u64, u64)>,
}
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct ProcessKey {
    start: u64,
    pid: u32,
}

fn u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_ne_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn u64_at(bytes: &[u8], offset: usize) -> Option<u64> {
    Some(u64::from_ne_bytes(
        bytes.get(offset..offset + 8)?.try_into().ok()?,
    ))
}

fn process_key(bytes: &[u8]) -> Option<ProcessKey> {
    Some(ProcessKey {
        start: u64_at(bytes, 0)?,
        pid: u32_at(bytes, 8)?,
    })
}
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
        Ok(Self {
            _links: links,
            obj,
            previous_cpu: HashMap::new(),
        })
    }

    pub(super) fn collect(
        &mut self,
        now: u64,
        snapshot: &super::snapshot::SystemSnapshot,
    ) -> Result<Vec<CpuActivity>> {
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

        let mut running: HashMap<ProcessKey, Vec<usize>> = HashMap::new();
        let mut ongoing: HashMap<ProcessKey, u64> = HashMap::new();
        for (cpu, value) in current.iter().enumerate() {
            let Some(key) = process_key(value) else {
                continue;
            };
            let since = u64_at(value, 16).unwrap_or(now);
            if key.pid == 0 || since > now {
                continue;
            }
            running.entry(key).or_default().push(cpu);
            *ongoing.entry(key).or_default() += now - since;
        }

        let mut cumulative: HashMap<ProcessKey, (u64, u64)> = HashMap::new();
        for key_bytes in totals.keys().take(65_536) {
            let Some(key) = process_key(&key_bytes) else {
                continue;
            };
            let Some(values) = totals.lookup_percpu(&key_bytes, MapFlags::ANY)? else {
                continue;
            };
            let value = cumulative.entry(key).or_default();
            for per_cpu in values {
                value.0 = value.0.saturating_add(u64_at(&per_cpu, 0).unwrap_or(0));
                value.1 = value.1.saturating_add(u64_at(&per_cpu, 8).unwrap_or(0));
            }
        }
        for (key, runtime) in ongoing {
            cumulative.entry(key).or_default().0 = cumulative
                .get(&key)
                .map_or(runtime, |value| value.0.saturating_add(runtime));
        }

        let ticks = crate::http_server::process::ticks_per_second() as u128;
        let known: HashMap<_, _> = snapshot
            .processes
            .iter()
            .map(|process| (process.identity, process))
            .collect();
        let mut result = Vec::new();
        let mut next_previous = HashMap::new();
        for (key, total) in cumulative {
            let identity = crate::http_server::process::ProcessId {
                pid: key.pid as i32,
                start_time_ticks: ((key.start as u128 * ticks) / 1_000_000_000) as u64,
            };
            if !known.contains_key(&identity) {
                continue;
            }
            let previous = self.previous_cpu.get(&key).copied().unwrap_or(total);
            let runtime_ns = total.0.saturating_sub(previous.0);
            let switches = total.1.saturating_sub(previous.1);
            let cpus = running.remove(&key).unwrap_or_default();
            if runtime_ns > 0 || switches > 0 || !cpus.is_empty() {
                result.push(CpuActivity {
                    process_id: identity,
                    runtime_ns,
                    switches,
                    running_threads: cpus.len(),
                    cpus,
                });
            }
            next_previous.insert(key, total);
        }
        self.previous_cpu = next_previous;
        Ok(result)
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
