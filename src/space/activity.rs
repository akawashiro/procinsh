use super::Space;
use anyhow::{Context, Result};
use libbpf_rs::{MapCore, MapFlags, ObjectBuilder, RingBufferBuilder};
use serde_json::json;
use std::{
    collections::HashMap,
    ffi::c_void,
    ptr::NonNull,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Sample {
    time: u64,
    addr: u64,
    source: u64,
    pid: u32,
    tid: u32,
}
unsafe extern "C" {
    fn procinsh_ibs_open(tid: i32, kind: i32, period: u64) -> *mut c_void;
    fn procinsh_ibs_close(handle: *mut c_void);
    fn procinsh_ibs_period(handle: *mut c_void, period: u64) -> i32;
    fn procinsh_ibs_poll(handle: *mut c_void, out: *mut Sample, cap: i32, lost: *mut u64) -> i32;
}
struct Ibs(NonNull<c_void>);
impl Drop for Ibs {
    fn drop(&mut self) {
        unsafe { procinsh_ibs_close(self.0.as_ptr()) }
    }
}
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
type ThreadKey = (crate::process::ProcessId, i32, u64);
fn selected_threads(
    targets: &std::collections::HashSet<crate::process::ProcessId>,
) -> std::collections::HashSet<ThreadKey> {
    let mut result = std::collections::HashSet::new();
    for id in targets {
        if crate::process::check_identity(*id).is_err() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(format!("/proc/{}/task", id.pid)) else {
            continue;
        };
        for entry in entries.flatten() {
            if let Ok(stat) =
                crate::process::procfs::read_stat(&entry.path().join("stat").to_string_lossy())
            {
                result.insert((*id, stat.pid, stat.start_time));
            }
        }
        if crate::process::check_identity(*id).is_err() {
            result.retain(|(owner, _, _)| owner != id);
        }
    }
    result
}
fn ibs(thread: ThreadKey, period: u64) -> Result<Ibs> {
    let (owner, tid, start) = thread;
    crate::process::check_identity(owner)?;
    let kind = std::fs::read_to_string("/sys/bus/event_source/devices/ibs_op/type")?
        .trim()
        .parse()?;
    let ptr = NonNull::new(unsafe { procinsh_ibs_open(tid, kind, period) }).with_context(|| {
        format!(
            "IBS thread {tid}: {} (CAP_PERFMON required)",
            std::io::Error::last_os_error()
        )
    })?;
    let handle = Ibs(ptr);
    let stat = crate::process::procfs::read_stat(&format!("/proc/{}/task/{tid}/stat", owner.pid))?;
    anyhow::ensure!(stat.start_time == start, "Thread identity changed");
    crate::process::check_identity(owner)?;
    Ok(handle)
}
pub fn run(space: Arc<Space>) {
    let mut bpf = None;
    let mut files: Option<super::files::Files> = None;
    let mut perf = HashMap::<ThreadKey, Ibs>::new();
    let mut targets = std::collections::HashSet::new();
    let mut threads_at = Instant::now() - Duration::from_secs(1);
    let mut current = 0;
    let mut last = Instant::now();
    let mut lost = 0;
    let mut prior_lost = 0;
    let mut effective_period = 250_000;
    let mut adjust_at = Instant::now();
    let mut invalidated = std::collections::HashMap::<i32, u64>::new();
    let mut unresolved = 0u64;
    let mut memory = std::collections::HashMap::new();
    let mut comm = std::collections::HashMap::new();
    let mut samples = [Sample::default(); 4096];
    let mut logged = super::StatusLog::default();
    log::info!("SPACE activity worker started");
    while !space.stopped() {
        let density = space.density();
        if density == 0 {
            logged.observe(&json!({"ipc":"idle", "cpu":"idle", "files":"idle", "memory":"idle"}));
            bpf = None;
            files = None;
            perf.clear();
            targets.clear();
            threads_at = Instant::now() - Duration::from_secs(1);
            current = 0;
            memory.clear();
            comm.clear();
            std::thread::sleep(Duration::from_millis(100));
            continue;
        }
        if current != density {
            log::info!("SPACE observation active density={density}");
            perf.clear();
            threads_at = Instant::now() - Duration::from_secs(1);
            let period = match density {
                1 => 1_000_000,
                3 => 100_000,
                _ => 250_000,
            };
            effective_period = period;
            invalidated.clear();
            let mut status = json!({"active":true,"density":density,"period":period,"ipc":"starting","memory":"starting","cpu":"starting","coverage":"pipe read/write; socket send/recv. splice, sendfile and some io_uring paths are not observed; worker attribution is excluded."});
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
            *space.status.lock().unwrap() = status;
            current = density;
            lost = 0;
            unresolved = 0;
        }
        if let Some(sensor) = &files {
            space.status.lock().unwrap()["files"] = match sensor.poll() {
                Ok(()) => json!("observing"),
                Err(error) => json!(format!("error: {error:#}")),
            };
        }
        let requested = space.memory_targets();
        if requested != targets || threads_at.elapsed() >= Duration::from_millis(250) {
            targets = requested;
            let threads = selected_threads(&targets);
            perf.retain(|key, _| threads.contains(key));
            memory.retain(|(id, _, _, _), _| targets.contains(id));
            let mut failure = None;
            // Use a stable representative error so HashSet iteration cannot flood logs.
            let mut threads: Vec<_> = threads.into_iter().collect();
            threads.sort_by_key(|(owner, tid, start)| (owner.pid, *tid, *start));
            for thread in threads {
                if perf.contains_key(&thread) {
                    continue;
                }
                match ibs(thread, effective_period) {
                    Ok(handle) => {
                        perf.insert(thread, handle);
                    }
                    Err(error) => {
                        failure.get_or_insert_with(|| format!("unavailable: {error:#}"));
                    }
                }
            }
            let mut status = space.status.lock().unwrap();
            status["memory"] = json!(failure.unwrap_or_else(|| if perf.is_empty() {
                "idle".into()
            } else {
                "sampling".into()
            }));
            status["memory_threads"] = json!(perf.len());
            threads_at = Instant::now();
        }
        let topology = space.snapshot.read().unwrap().clone();
        let nodes: std::collections::HashMap<_, _> =
            topology.nodes.iter().map(|n| (n.identity.pid, n)).collect();
        if let Some(b) = &mut bpf {
            let consumed = b.ring.consume_raw_n(8192);
            if consumed < 0 {
                let e = std::io::Error::from_raw_os_error(-consumed);
                space.status.lock().unwrap()["ipc"] = json!(format!("error: {e}"));
            } else {
                space.status.lock().unwrap()["ipc"] = json!("observing");
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
        let mut valid = std::collections::HashMap::new();
        for p in perf.values() {
            let count = unsafe {
                procinsh_ibs_poll(
                    p.0.as_ptr(),
                    samples.as_mut_ptr(),
                    samples.len() as i32,
                    &mut lost,
                )
            };
            for sample in &samples[..count.max(0) as usize] {
                let Some(n) = nodes.get(&(sample.pid as i32)) else {
                    unresolved += 1;
                    continue;
                };
                if !targets.contains(&n.identity) {
                    continue;
                }
                if sample.source == u64::MAX {
                    invalidated
                        .entry(sample.pid as i32)
                        .and_modify(|t| *t = (*t).max(sample.time))
                        .or_insert(sample.time);
                    continue;
                }
                if sample.time < n.maps_epoch
                    || invalidated
                        .get(&(sample.pid as i32))
                        .is_some_and(|&t| t >= n.maps_epoch)
                {
                    unresolved += 1;
                    continue;
                }
                // The kernel sample's TID/PID are checked against current identity before assigning a map.
                if sample.addr == 0 || !n.maps.iter().any(|m| m.contains(sample.addr)) {
                    unresolved += 1;
                    continue;
                }
                if !*valid
                    .entry(n.identity)
                    .or_insert_with(|| crate::process::check_identity(n.identity).is_ok())
                {
                    unresolved += 1;
                    continue;
                }
                if memory.len() >= 8192 {
                    unresolved += 1;
                    continue;
                }
                let op = sample.source & 31;
                let mode = if op & 4 != 0 {
                    "write"
                } else if op & 2 != 0 {
                    "read"
                } else {
                    "unknown"
                };
                *memory
                    .entry((n.identity, n.maps_epoch, sample.addr & !4095, mode))
                    .or_insert(0u64) += 1;
            }
        }
        if last.elapsed() >= Duration::from_millis(100) {
            memory.retain(|(id, epoch, _, _), _| {
                invalidated.get(&id.pid).is_none_or(|t| *t < *epoch)
            });
            let mem:Vec<_>=memory.drain().map(|((id,map_epoch,page,mode),count)|json!({"process_id":id,"maps_epoch":map_epoch,"page":format!("0x{page:016x}"),"mode":mode,"count":count})).collect();
            let ipc:Vec<_>=comm.drain().map(|((id,resource,write),(bytes,count))|json!({"process_id":id,"resource":resource,"write":write,"bytes":bytes,"count":count})).collect();
            let cpu = if let Some(bpf) = &mut bpf {
                match bpf.cpu_activity(super::monotonic_ns(), &topology) {
                    Ok(activity) => {
                        space.status.lock().unwrap()["cpu"] = json!("observing");
                        activity
                    }
                    Err(error) => {
                        space.status.lock().unwrap()["cpu"] = json!(format!("error: {error:#}"));
                        Vec::new()
                    }
                }
            } else {
                Vec::new()
            };
            if adjust_at.elapsed() >= Duration::from_secs(2) {
                if lost > prior_lost && effective_period < 1_000_000 {
                    let next = (effective_period * 2).min(1_000_000);
                    let ok = perf
                        .values()
                        .all(|p| unsafe { procinsh_ibs_period(p.0.as_ptr(), next) } == 0);
                    if ok {
                        effective_period = next;
                    } else {
                        perf.clear();
                        space.status.lock().unwrap()["memory"] =
                            json!("unavailable: IBS period update failed; sampling stopped");
                    }
                }
                prior_lost = lost;
                adjust_at = Instant::now();
            }
            invalidated.retain(|pid, time| nodes.get(pid).is_some_and(|n| *time >= n.maps_epoch));
            let mut status = space.status.lock().unwrap();
            status["lost"] = json!(lost + bpf.as_ref().map_or(0, Bpf::lost));
            status["unresolved"] = json!(unresolved);
            status["period"] = json!(effective_period);
            status["files_lost"] = json!(files.as_ref().map_or(0, |sensor| sensor.lost()));
            let file_events = files
                .as_ref()
                .map_or_else(Vec::new, |sensor| sensor.drain());
            space.send("activity",json!({"captured_at":crate::process::timestamp_ms(),"window_ms":last.elapsed().as_millis(),"files":file_events,"memory":mem,"ipc":ipc,"cpu":cpu,"invalidated":invalidated.keys().collect::<Vec<_>>(),"status":*status}));
            last = Instant::now();
        }
        logged.observe(&space.status.lock().unwrap());
        std::thread::sleep(Duration::from_millis(10));
    }
    log::info!("SPACE activity worker stopped");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn selected_thread_identity_and_lifecycle() {
        let id = crate::process::identity(std::process::id() as i32).unwrap();
        let targets = [id].into_iter().collect();
        let before = selected_threads(&targets);
        assert!(
            before
                .iter()
                .any(|(owner, tid, _)| *owner == id && *tid == id.pid)
        );
        let (tx, rx) = std::sync::mpsc::channel();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            ready_tx
                .send(unsafe { libc::syscall(libc::SYS_gettid) } as i32)
                .unwrap();
            rx.recv().unwrap();
        });
        let tid = ready_rx.recv().unwrap();
        assert!(selected_threads(&targets).iter().any(|(_, t, _)| *t == tid));
        tx.send(()).unwrap();
        thread.join().unwrap();
        let fake = crate::process::ProcessId {
            start_time_ticks: id.start_time_ticks + 1,
            ..id
        };
        assert!(selected_threads(&[fake].into_iter().collect()).is_empty());
        assert!(selected_threads(&std::collections::HashSet::new()).is_empty());
    }
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
