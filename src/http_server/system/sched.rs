//! Derives process CPU activity from per-CPU scheduler maps.
//!
//! # Interface
//!
//! - [`CpuActivityCollector`] (`pub(super) struct`): owns the sensor and computes activity deltas.
use super::bpf::wire::{CpuSlot, CpuTotal, ProcessKey};
use super::model::CpuActivity;
use anyhow::{Context, Result};
use aya::maps::{PerCpuArray, PerCpuHashMap};
use std::collections::HashMap;
/// Owns BPF resources and derives CPU activity from scheduler counters.
pub(super) struct CpuActivityCollector {
    obj: aya::Ebpf,
    previous_cpu: HashMap<ProcessKey, (u64, u64)>,
}
impl CpuActivityCollector {
    pub(super) fn new() -> Result<Self> {
        let obj = super::bpf::load("sched")?;
        Ok(Self {
            obj,
            previous_cpu: HashMap::new(),
        })
    }

    pub(super) fn collect(
        &mut self,
        now: u64,
        snapshot: &super::snapshot::SystemSnapshot,
    ) -> Result<Vec<CpuActivity>> {
        let current = PerCpuArray::<_, CpuSlot>::try_from(
            self.obj.map("cpu_current").context("cpu_current map")?,
        )?
        .get(&0, 0)?;
        let totals = PerCpuHashMap::<_, ProcessKey, CpuTotal>::try_from(
            self.obj.map("cpu_totals").context("cpu_totals map")?,
        )?;

        let mut running: HashMap<ProcessKey, Vec<usize>> = HashMap::new();
        let mut ongoing: HashMap<ProcessKey, u64> = HashMap::new();
        for (cpu, value) in current.iter().enumerate() {
            let key = value.process;
            let since = value.since;
            if key.pid == 0 || since > now {
                continue;
            }
            running.entry(key).or_default().push(cpu);
            *ongoing.entry(key).or_default() += now - since;
        }

        let mut cumulative: HashMap<ProcessKey, (u64, u64)> = HashMap::new();
        for key in totals.keys().take(65_536) {
            let key = key?;
            let values = match totals.get(&key, 0) {
                Ok(values) => values,
                Err(aya::maps::MapError::KeyNotFound) => continue,
                Err(error) => return Err(error.into()),
            };
            let value = cumulative.entry(key).or_default();
            for per_cpu in values.iter() {
                value.0 = value.0.saturating_add(per_cpu.runtime);
                value.1 = value.1.saturating_add(per_cpu.switches);
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
