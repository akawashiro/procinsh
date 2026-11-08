//! Derive system activity from bounded kernel observations in main.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`FileBatch`] | `pub(crate)` | `struct FileBatch` |
//! | [`ActivityBatch`] | `pub(crate)` | `struct ActivityBatch` |
//! | [`ActivityProcessor`] | `pub(in crate::http_server)` | `struct ActivityProcessor` |
//! | [`add_file_batch`] | `pub(crate)` | `fn add_file_batch(state: &mut `[`FileBatch`]`, event: `[`FileActivity`]`)` |
//! | [`resolve_signal`] | `pub(crate)` | `fn resolve_signal( sample: `[`SignalSample`]`, processes: &`[`HashMap`]`<i32, `[`ProcessId`]`>, ) -> `[`Option`]`<`[`SignalEvent`]`>` |
//! | [`process_activity_processor`] | `pub(in crate::http_server)` | `fn process_activity_processor( state: &mut `[`ActivityProcessor`]`, raw: `[`RawActivity`]`, known: &`[`HashMap`]`<i32, `[`ProcessId`]`>, ) -> `[`ActivityBatch`] |

use crate::ipc::activity::{
    CpuActivity, CpuCounters, FileActivity, IpcActivity, IpcEvent, ProcessKey, RawActivity,
    SignalEvent, SignalSample,
};
use crate::ipc::activity::{process_key, u64_at};
use crate::ipc::{
    model::{
        process::ticks_per_second,
        resource::{FileIdentity, IpcIdentity, IpcKind},
    },
    schema::ProcessId,
};
use std::collections::HashMap;
#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::activity::SignalEvent")]
struct SignalEventJson {
    timestamp_ns: u64,
    src_pid: u32,
    dst_pid: u32,
    signal: i32,
    source_id: ProcessId,
    destination_id: ProcessId,
}
impl serde::Serialize for crate::ipc::activity::SignalEvent {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        SignalEventJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::activity::SignalEvent {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        SignalEventJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::activity::IpcActivity")]
struct IpcActivityJson {
    process_id: ProcessId,
    resource: IpcIdentity,
    write: bool,
    bytes: u64,
    count: u64,
}
impl serde::Serialize for crate::ipc::activity::IpcActivity {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        IpcActivityJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::activity::IpcActivity {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        IpcActivityJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::activity::CpuActivity")]
struct CpuActivityJson {
    process_id: ProcessId,
    runtime_ns: u64,
    switches: u64,
    running_threads: usize,
    cpus: Vec<usize>,
}
impl serde::Serialize for crate::ipc::activity::CpuActivity {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        CpuActivityJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::activity::CpuActivity {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        CpuActivityJson::deserialize(d)
    }
}

const MAX_AGGREGATES: usize = 8192;

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::activity::FileActivity")]
struct FileActivityJson {
    process_id: ProcessId,
    file: FileIdentity,
    path: Option<String>,
    write: bool,
    bytes: u64,
    count: u64,
}
impl serde::Serialize for crate::ipc::activity::FileActivity {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        FileActivityJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::activity::FileActivity {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        FileActivityJson::deserialize(d)
    }
}

#[derive(Default)]
pub(crate) struct FileBatch {
    pub(crate) events: HashMap<(ProcessId, FileIdentity, bool), FileActivity>,
    pub(crate) dropped: u64,
}
pub(crate) fn add_file_batch(state: &mut FileBatch, event: FileActivity) {
    let key = (event.process_id, event.file, event.write);
    if let Some(prior) = state.events.get_mut(&key) {
        prior.bytes = prior.bytes.saturating_add(event.bytes);
        prior.count = prior.count.saturating_add(event.count);
        if event.path.is_some() {
            prior.path = event.path;
        }
    } else if state.events.len() < MAX_AGGREGATES {
        state.events.insert(key, event);
    } else {
        state.dropped += 1;
    }
}
#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::activity::IpcEvent")]
struct IpcEventJson {
    start: u64,
    inode: u64,
    device: u64,
    bytes: u64,
    pid: u32,
    kind: u32,
    write: bool,
    worker: bool,
}
impl serde::Serialize for crate::ipc::activity::IpcEvent {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        IpcEventJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::activity::IpcEvent {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        IpcEventJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::activity::SignalSample")]
struct SignalSampleJson {
    timestamp_ns: u64,
    src_start: u64,
    dst_start: u64,
    src_pid: u32,
    dst_pid: u32,
    signal: i32,
}
impl serde::Serialize for crate::ipc::activity::SignalSample {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        SignalSampleJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::activity::SignalSample {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        SignalSampleJson::deserialize(d)
    }
}

pub(crate) fn resolve_signal(
    sample: SignalSample,
    processes: &HashMap<i32, ProcessId>,
) -> Option<SignalEvent> {
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

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::activity::CpuCounters")]
struct CpuCountersJson {
    now: u64,
    current: Vec<Vec<u8>>,
    /// Nonzero per-CPU cumulative runtime/switch pairs; CPU indices are in `current`.
    totals: Vec<(Vec<u8>, Vec<[u64; 2]>)>,
}
impl serde::Serialize for crate::ipc::activity::CpuCounters {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        CpuCountersJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::activity::CpuCounters {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        CpuCountersJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::activity::RawActivity")]
struct RawActivityJson {
    files: Vec<FileActivity>,
    ipc: Vec<IpcEvent>,
    signals: Vec<SignalSample>,
    cpu: CpuCounters,
}
impl serde::Serialize for crate::ipc::activity::RawActivity {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        RawActivityJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::activity::RawActivity {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        RawActivityJson::deserialize(d)
    }
}

pub(crate) struct ActivityBatch {
    pub(in crate::http_server) files: Vec<FileActivity>,
    pub(in crate::http_server) ipc: Vec<IpcActivity>,
    pub(in crate::http_server) signals: Vec<SignalEvent>,
    pub(in crate::http_server) cpu: Vec<CpuActivity>,
    pub(in crate::http_server) unresolved: u64,
    pub(in crate::http_server) files_dropped: u64,
}
#[derive(Default)]
pub(in crate::http_server) struct ActivityProcessor {
    previous_cpu: HashMap<ProcessKey, (u64, u64)>,
    previous_running: std::collections::HashSet<ProcessKey>,
    unresolved: u64,
}
pub(in crate::http_server) fn process_activity_processor(
    state: &mut ActivityProcessor,
    raw: RawActivity,
    known: &HashMap<i32, ProcessId>,
) -> ActivityBatch {
    use crate::ipc::model::resource::device_id_from_kernel;

    let mut files = FileBatch::default();
    for event in raw.files {
        add_file_batch(&mut files, event);
    }
    let mut pending = HashMap::new();
    for e in raw.ipc {
        let ticks = (e.start as u128 * ticks_per_second() as u128 / 1_000_000_000) as u64;
        let Some(id) = known
            .get(&(e.pid as i32))
            .filter(|id| !e.worker && ticks == id.start_time_ticks)
        else {
            state.unresolved += 1;
            continue;
        };
        let resource = IpcIdentity {
            kind: if e.kind == 1 {
                IpcKind::Pipe
            } else {
                IpcKind::Socket
            },
            device: device_id_from_kernel(e.device),
            inode: e.inode,
        };
        if pending.len() >= 8192 {
            state.unresolved += 1;
            continue;
        }
        let entry = pending
            .entry((*id, resource, e.write))
            .or_insert((0u64, 0u64));
        entry.0 = entry.0.saturating_add(e.bytes);
        entry.1 += 1;
    }
    let ipc = pending
        .into_iter()
        .map(
            |((process_id, resource, write), (bytes, count))| IpcActivity {
                process_id,
                resource,
                write,
                bytes,
                count,
            },
        )
        .collect();
    let signals = raw
        .signals
        .into_iter()
        .filter_map(|sample| resolve_signal(sample, known))
        .collect();
    let cpu = cpu_activity_processor(state, raw.cpu, known);
    ActivityBatch {
        files: files.events.into_values().collect(),
        ipc,
        signals,
        cpu,
        unresolved: state.unresolved,
        files_dropped: files.dropped,
    }
}

fn cpu_activity_processor(
    state: &mut ActivityProcessor,
    raw: CpuCounters,
    known: &HashMap<i32, ProcessId>,
) -> Vec<CpuActivity> {
    let now = raw.now;
    let mut running: HashMap<ProcessKey, Vec<usize>> = HashMap::new();
    let mut ongoing: HashMap<ProcessKey, u64> = HashMap::new();
    for (cpu, value) in raw.current.iter().enumerate() {
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
    for (key_bytes, values) in raw.totals {
        let Some(key) = process_key(&key_bytes) else {
            continue;
        };
        let value = cumulative.entry(key).or_default();
        for per_cpu in values {
            value.0 = value.0.saturating_add(per_cpu[0]);
            value.1 = value.1.saturating_add(per_cpu[1]);
        }
    }
    for (key, runtime) in ongoing {
        cumulative.entry(key).or_default().0 = cumulative
            .get(&key)
            .map_or(runtime, |value| value.0.saturating_add(runtime));
    }

    let ticks = ticks_per_second() as u128;
    let known: std::collections::HashSet<_> = known.values().copied().collect();
    let mut result = Vec::new();
    let mut next_previous = HashMap::new();
    let mut next_running = std::collections::HashSet::new();
    for (key, total) in cumulative {
        let identity = ProcessId {
            pid: key.pid as i32,
            start_time_ticks: ((key.start as u128 * ticks) / 1_000_000_000) as u64,
        };
        if !known.contains(&identity) {
            continue;
        }
        let previous = state.previous_cpu.get(&key).copied().unwrap_or(total);
        let runtime_ns = total.0.saturating_sub(previous.0);
        let switches = total.1.saturating_sub(previous.1);
        let cpus = running.remove(&key).unwrap_or_default();
        if !cpus.is_empty() {
            next_running.insert(key);
        }
        if runtime_ns > 0
            || switches > 0
            || !cpus.is_empty()
            || state.previous_running.contains(&key)
        {
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
    state.previous_cpu = next_previous;
    state.previous_running = next_running;
    result
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::ipc::activity::{CpuCounters, RawActivity},
    };
    #[test]
    fn cpu_idle_transition_is_reported_even_without_a_counter_delta() {
        let start = 1_000_000_000u64;
        let id = ProcessId {
            pid: 42,
            start_time_ticks: ticks_per_second() as u64,
        };
        let known = HashMap::from([(42, id)]);
        let mut key = vec![0; 16];
        key[..8].copy_from_slice(&start.to_ne_bytes());
        key[8..12].copy_from_slice(&42u32.to_ne_bytes());
        let now = start + 1_000_000_000;
        let mut slot = vec![0; 32];
        slot[..16].copy_from_slice(&key);
        slot[16..24].copy_from_slice(&now.to_ne_bytes());
        let captured = |current| RawActivity {
            cpu: CpuCounters {
                now,
                current,
                totals: vec![(key.clone(), vec![[100, 2]])],
            },
            ..Default::default()
        };
        let mut processor = ActivityProcessor::default();
        let busy =
            super::process_activity_processor(&mut processor, captured(vec![slot]), &known).cpu;
        assert_eq!(busy[0].running_threads, 1);
        let idle =
            super::process_activity_processor(&mut processor, captured(Vec::new()), &known).cpu;
        assert_eq!(idle.len(), 1);
        assert_eq!(idle[0].running_threads, 0);
        assert_eq!(idle[0].runtime_ns, 0);
        assert_eq!(idle[0].switches, 0);
        assert!(idle[0].cpus.is_empty());
        assert!(
            super::process_activity_processor(&mut processor, captured(Vec::new()), &known)
                .cpu
                .is_empty()
        );
    }

    #[test]
    fn cpu_deltas_include_work_completed_before_a_process_became_idle() {
        let start = 1_000_000_000u64;
        let id = ProcessId {
            pid: 42,
            start_time_ticks: ticks_per_second() as u64,
        };
        let known = HashMap::from([(42, id)]);
        let mut key = vec![0; 16];
        key[..8].copy_from_slice(&start.to_ne_bytes());
        key[8..12].copy_from_slice(&42u32.to_ne_bytes());
        let counters = |values| RawActivity {
            cpu: CpuCounters {
                now: 10,
                current: Vec::new(),
                totals: vec![(key.clone(), values)],
            },
            ..Default::default()
        };
        let mut processor = ActivityProcessor::default();
        assert!(
            super::process_activity_processor(&mut processor, counters(vec![[100, 2]]), &known)
                .cpu
                .is_empty()
        );
        let cpu = super::process_activity_processor(
            &mut processor,
            counters(vec![[150, 3], [20, 2]]),
            &known,
        )
        .cpu;
        assert_eq!(cpu.len(), 1);
        assert_eq!(cpu[0].runtime_ns, 70);
        assert_eq!(cpu[0].switches, 3);
        assert_eq!(cpu[0].running_threads, 0);
        assert!(cpu[0].cpus.is_empty());
        assert!(
            super::process_activity_processor(
                &mut processor,
                counters(vec![[150, 3], [20, 2]]),
                &known
            )
            .cpu
            .is_empty()
        );
    }
}
