use super::System;
use anyhow::{Context, Result};
use libbpf_rs::{MapCore, MapFlags, ObjectBuilder, RingBufferBuilder};
use serde_json::json;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
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
struct Bpf {
    ring: libbpf_rs::RingBuffer<'static>,
    _links: Vec<libbpf_rs::Link>,
    obj: libbpf_rs::Object,
    queue: Arc<Mutex<Vec<Event>>>,
    drops: Arc<std::sync::atomic::AtomicU64>,
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
impl Bpf {
    fn new() -> Result<Self> {
        let open = ObjectBuilder::default()
            .open_memory(include_bytes!(concat!(env!("OUT_DIR"), "/activity.bpf.o")))?;
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
            previous_cpu: HashMap::new(),
        })
    }
    fn lost(&self) -> u64 {
        self.obj
            .maps()
            .find(|m| m.name() == "lost")
            .and_then(|m| m.lookup(&0u32.to_ne_bytes(), MapFlags::ANY).ok().flatten())
            .and_then(|v| v.try_into().ok())
            .map(u64::from_ne_bytes)
            .unwrap_or(0)
            + self.drops.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn cpu_activity(
        &mut self,
        now: u64,
        topology: &super::topology::Topology,
    ) -> Result<Vec<serde_json::Value>> {
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

        let ticks = crate::process::procfs::ticks_per_second() as u128;
        let known: HashMap<_, _> = topology
            .nodes
            .iter()
            .map(|node| (node.identity, node))
            .collect();
        let mut result = Vec::new();
        let mut next_previous = HashMap::new();
        for (key, total) in cumulative {
            let identity = crate::process::ProcessId {
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
                result.push(json!({
                    "process_id": identity,
                    "runtime_ns": runtime_ns,
                    "switches": switches,
                    "running_threads": cpus.len(),
                    "cpus": cpus,
                }));
            }
            next_previous.insert(key, total);
        }
        self.previous_cpu = next_previous;
        Ok(result)
    }
}
pub fn run(system: Arc<System>) {
    let mut bpf = None;
    let mut files: Option<super::files::Files> = None;
    let mut active = false;
    let mut last = Instant::now();
    let mut unresolved = 0u64;
    let mut comm = std::collections::HashMap::new();
    let mut logged = super::StatusLog::default();
    log::info!("SPACE activity worker started");
    while !system.stopped() {
        if !system.active() {
            logged.observe(&json!({"ipc":"idle", "cpu":"idle", "files":"idle"}));
            bpf = None;
            files = None;
            active = false;
            comm.clear();
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        if !active {
            log::info!("SPACE observation active");
            let mut status = json!({"active":true,"ipc":"starting","cpu":"starting","coverage":"pipe read/write; socket send/recv. splice, sendfile and some io_uring paths are not observed; worker attribution is excluded."});
            if bpf.is_none() {
                match Bpf::new() {
                    Ok(v) => {
                        bpf = Some(v);
                        status["ipc"] = json!("observing");
                        status["cpu"] = json!("observing");
                    }
                    Err(e) => {
                        let message = json!(format!("unavailable: {e:#}"));
                        status["ipc"] = message.clone();
                        status["cpu"] = message;
                    }
                }
            } else {
                status["ipc"] = json!("observing");
                status["cpu"] = json!("observing");
            }
            if files.is_none() {
                match super::files::Files::new() {
                    Ok(sensor) => {
                        files = Some(sensor);
                        status["files"] = json!("observing");
                    }
                    Err(error) => {
                        status["files"] = json!(format!("unavailable: {error:#}"));
                    }
                }
            } else {
                status["files"] = json!("observing");
            }
            status["files_coverage"] = json!(super::files::COVERAGE);
            *system.status.lock().unwrap() = status;
            active = true;
            unresolved = 0;
        }
        if let Some(sensor) = &files {
            system.status.lock().unwrap()["files"] = match sensor.poll() {
                Ok(()) => json!("observing"),
                Err(error) => json!(format!("error: {error:#}")),
            };
        }
        let topology = system.snapshot.read().unwrap().clone();
        let nodes: std::collections::HashMap<_, _> =
            topology.nodes.iter().map(|n| (n.identity.pid, n)).collect();
        if let Some(b) = &mut bpf {
            let consumed = b.ring.consume_raw_n(8192);
            if consumed < 0 {
                let e = std::io::Error::from_raw_os_error(-consumed);
                system.status.lock().unwrap()["ipc"] = json!(format!("error: {e}"));
            } else {
                system.status.lock().unwrap()["ipc"] = json!("observing");
            }
            for e in b.queue.lock().unwrap().drain(..) {
                let Some(n) = nodes.get(&(e.pid as i32)) else {
                    unresolved += 1;
                    continue;
                };
                let ticks = ((e.start as u128 * crate::process::procfs::ticks_per_second() as u128)
                    / 1_000_000_000) as u64;
                if e.worker || ticks != n.identity.start_time_ticks {
                    unresolved += 1;
                    continue;
                }
                let dev =
                    libc::makedev((e.device >> 20) as u32, (e.device & ((1 << 20) - 1)) as u32);
                let resource = super::topology::resource(
                    if e.kind == 1 { "pipe" } else { "socket" },
                    dev,
                    e.inode,
                );
                if comm.len() >= 8192 {
                    unresolved += 1;
                    continue;
                }
                let value = comm
                    .entry((n.identity, resource, e.write))
                    .or_insert((0u64, 0u64));
                value.0 += e.bytes;
                value.1 += 1;
            }
        }
        if last.elapsed() >= Duration::from_millis(100) {
            let ipc:Vec<_>=comm.drain().map(|((id,resource,write),(bytes,count))|json!({"process_id":id,"resource":resource,"write":write,"bytes":bytes,"count":count})).collect();
            let cpu = if let Some(bpf) = &mut bpf {
                match bpf.cpu_activity(super::monotonic_ns(), &topology) {
                    Ok(activity) => {
                        system.status.lock().unwrap()["cpu"] = json!("observing");
                        activity
                    }
                    Err(error) => {
                        system.status.lock().unwrap()["cpu"] = json!(format!("error: {error:#}"));
                        Vec::new()
                    }
                }
            } else {
                Vec::new()
            };
            let mut status = system.status.lock().unwrap();
            status["lost"] = json!(bpf.as_ref().map_or(0, Bpf::lost));
            status["unresolved"] = json!(unresolved);
            status["files_lost"] = json!(files.as_ref().map_or(0, |sensor| sensor.lost()));
            let file_events = files
                .as_ref()
                .map_or_else(Vec::new, |sensor| sensor.drain());
            system.send("activity",json!({"captured_at":crate::process::timestamp_ms(),"window_ms":last.elapsed().as_millis(),"files":file_events,"ipc":ipc,"cpu":cpu,"status":*status}));
            last = Instant::now();
        }
        logged.observe(&system.status.lock().unwrap());
        std::thread::sleep(Duration::from_millis(10));
    }
    log::info!("SPACE activity worker stopped");
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
