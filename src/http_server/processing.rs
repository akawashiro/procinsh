//! Interpret acquisition replies before HTTP output in the unprivileged process.
//!
//! # Interface
//! [`ObservationProcessor`] (`pub(super)`): `struct ObservationProcessor`.
use super::client::MonitorClient;
use crate::{
    http_server::analysis::snapshot::{
        registers::{RegisterSet, from_sample},
        symbol::{ElfCache, elf_address, instruction_address, resolve_frame},
        unwind::StackUnwinder,
    },
    http_server::model::memory::{MappingSnapshot, MemoryMap},
    http_server::protocol::{MonitorEvent, MonitorResponse, ProcessId},
};
use anyhow::Result;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

struct ProcessHistory {
    sample: crate::http_server::model::process::ProcessSample,
    observation: crate::http_server::model::process::ProcessObservation,
    points: std::collections::VecDeque<crate::http_server::analysis::history::HistoryPoint>,
}
/// Owns analysis caches and independent per-session histories and BPF deltas.
/// IPC-only capture fields are consumed before any HTTP serialization.
#[derive(Default)]
pub(super) struct ObservationProcessor {
    histories: HashMap<(u64, ProcessId), ProcessHistory>,
    activity: HashMap<u64, crate::http_server::analysis::activity::ActivityProcessor>,
    known: HashMap<i32, ProcessId>,
    discovery: HashMap<ProcessId, (u64, u64)>,
    symbols: ElfCache,
    loaded: HashSet<(ProcessId, u64, u64, u64)>,
    unwinders: HashMap<ProcessId, StackUnwinder>,
}
impl ObservationProcessor {
    pub(super) fn close(&mut self, session: u64) {
        let processes: Vec<_> = self
            .histories
            .keys()
            .filter(|(key, _)| *key == session)
            .map(|(_, id)| *id)
            .collect();
        self.histories.retain(|(key, _), _| *key != session);
        for id in processes {
            if !self.histories.keys().any(|(_, other)| *other == id) {
                self.unwinders.remove(&id);
                self.loaded.retain(|(process, _, _, _)| *process != id);
            }
        }
        self.activity.remove(&session);
    }

    fn snapshot(
        &mut self,
        mut snapshot: crate::http_server::model::system::SystemSnapshot,
    ) -> Result<Value> {
        self.known = snapshot
            .processes
            .iter()
            .map(|process| (process.identity.pid, process.identity))
            .collect();
        if crate::http_server::analysis::relations::build(&mut snapshot) {
            snapshot.warnings.push("Scan, mapping, or connection limit reached. Some process or FD information is missing.".into());
        }
        let sources = crate::http_server::analysis::hardware::HardwareSources::acquired(
            std::mem::take(&mut snapshot.raw_hardware),
        );
        let hardware =
            crate::http_server::analysis::hardware::HardwareObservation::observe(&sources);
        let mut handles = hardware.handles(&sources);
        for (process, fd, rdev) in std::mem::take(&mut snapshot.raw_device_handles) {
            handles.observe(process, fd, rdev);
        }
        let live = snapshot
            .processes
            .iter()
            .map(|process| process.identity)
            .collect();
        snapshot.device_handles = handles.finish(&live);
        snapshot.hardware_devices = hardware.devices;
        snapshot.hardware_paths = hardware.paths;
        let hostnames: HashMap<_, _> = std::mem::take(&mut snapshot.raw_hostnames)
            .into_iter()
            .collect();
        for relation in &mut snapshot.fd_relations {
            if let Some(socket) = &mut relation.socket
                && socket.network_peer
            {
                socket.remote_hostname = socket
                    .remote
                    .and_then(|address| hostnames.get(&address.ip).cloned());
            }
        }
        Ok(serde_json::to_value(snapshot)?)
    }
    pub(super) async fn process(
        &mut self,
        client: &MonitorClient,
        session: Option<u64>,
        response: &mut MonitorResponse,
    ) -> Result<()> {
        let session = match response {
            MonitorResponse::Started { session, .. } => Some(*session),
            _ => session,
        };
        let mut snapshot_processed = false;
        let event = match response {
            MonitorResponse::Started { initial, .. } => Some(initial),
            MonitorResponse::Event {
                event: Some(event), ..
            } => Some(event),
            _ => None,
        };
        if let Some(event) = event {
            let acquired = matches!(
                event,
                MonitorEvent::AcquiredSnapshot(_) | MonitorEvent::AcquisitionGap { .. }
            );
            if acquired {
                let raw = std::mem::replace(event, MonitorEvent::Snapshot(Value::Null));
                *event = match raw {
                    MonitorEvent::AcquiredSnapshot(snapshot) => MonitorEvent::Snapshot(
                        self.snapshot(std::sync::Arc::unwrap_or_clone(snapshot))?,
                    ),
                    MonitorEvent::AcquisitionGap { dropped, snapshot } => MonitorEvent::Gap {
                        dropped,
                        snapshot: self.snapshot(std::sync::Arc::unwrap_or_clone(snapshot))?,
                    },
                    _ => unreachable!(),
                };
                snapshot_processed = true;
            }
        }
        let value = match response {
            MonitorResponse::Data(value) => value,
            MonitorResponse::Started {
                initial:
                    MonitorEvent::Observation(value)
                    | MonitorEvent::Snapshot(value)
                    | MonitorEvent::Activity(value),
                ..
            }
            | MonitorResponse::Event {
                event:
                    Some(
                        MonitorEvent::Observation(value)
                        | MonitorEvent::Snapshot(value)
                        | MonitorEvent::Activity(value),
                    ),
                ..
            } => value,
            MonitorResponse::Started {
                initial: MonitorEvent::Gap { snapshot, .. },
                ..
            }
            | MonitorResponse::Event {
                event: Some(MonitorEvent::Gap { snapshot, .. }),
                ..
            } => snapshot,
            _ => return Ok(()),
        };
        crate::http_server::analysis::details::complete(value)?;
        let name = |record: &mut Value| {
            if let Some(object) = record.as_object_mut() {
                object.remove("raw_ticks");
                object.remove("raw_measured_ns");
            }
        };
        if let Some(list) = value.as_array_mut() {
            let mut next = HashMap::new();
            for record in list.iter_mut() {
                if let (Ok(id), Some(ticks), Some(now)) = (
                    serde_json::from_value::<ProcessId>(record["identity"].clone()),
                    record["raw_ticks"].as_u64(),
                    record["raw_measured_ns"].as_u64(),
                ) {
                    let cpu = self
                        .discovery
                        .get(&id)
                        .and_then(|(old, then)| ticks.checked_sub(*old).zip(now.checked_sub(*then)))
                        .filter(|(_, elapsed)| *elapsed > 0)
                        .map(|(delta, elapsed)| {
                            delta as f64
                                / crate::http_server::model::process::ticks_per_second()
                                / (elapsed as f64 / 1_000_000_000.0)
                                * 100.0
                        });
                    record["cpu_percent"] = serde_json::json!(cpu);
                    next.insert(id, (ticks, now));
                }
                name(record);
            }
            if !next.is_empty() {
                self.discovery = next;
                list.sort_by(|a, b| {
                    b["cpu_percent"]
                        .as_f64()
                        .unwrap_or_default()
                        .total_cmp(&a["cpu_percent"].as_f64().unwrap_or_default())
                        .then(
                            a["identity"]["pid"]
                                .as_i64()
                                .cmp(&b["identity"]["pid"].as_i64()),
                        )
                });
            }
        }
        if let Some(summary) = value.get_mut("summary") {
            name(summary);
        }
        if let Some(processes) = value.get_mut("processes").and_then(Value::as_array_mut) {
            for process in processes {
                name(process);
            }
        }
        if let Some(raw) = value
            .as_object_mut()
            .and_then(|value| value.remove("raw_activity"))
            && !raw.is_null()
        {
            let raw = serde_json::from_value(raw)?;
            let session = session.unwrap_or_default();
            if self.activity.len() >= 64 && !self.activity.contains_key(&session) {
                self.activity.clear();
            }
            let batch = self
                .activity
                .entry(session)
                .or_default()
                .process(raw, &self.known);
            value["files"] = serde_json::to_value(batch.files)?;
            value["ipc"] = serde_json::to_value(batch.ipc)?;
            value["signals"] = serde_json::to_value(batch.signals)?;
            value["cpu"] = serde_json::to_value(batch.cpu)?;
            value["status"]["unresolved"] = serde_json::json!(batch.unresolved);
            value["status"]["files_lost"] = serde_json::json!(
                value["status"]["files_lost"]
                    .as_u64()
                    .unwrap_or_default()
                    .saturating_add(batch.files_dropped)
            );
        }
        if let Some(raw) = value
            .as_object_mut()
            .and_then(|value| value.remove("process_sample"))
        {
            use crate::http_server::model::process::{
                ProcessSample, initial_observation, next_observation,
            };
            let current: ProcessSample = serde_json::from_value(raw)?;
            let mut observed = initial_observation(&current);
            if value.get("summary").is_some() {
                let key = (session.unwrap_or_default(), current.process_id);
                if self.histories.len() >= 64 && !self.histories.contains_key(&key) {
                    self.histories.clear();
                }
                let history = self.histories.entry(key).or_insert_with(|| ProcessHistory {
                    sample: current.clone(),
                    observation: observed.clone(),
                    points: Default::default(),
                });
                if current.measured_at > history.sample.measured_at {
                    observed = next_observation(&history.sample, &current);
                } else {
                    observed = history.observation.clone();
                }
                if history
                    .points
                    .back()
                    .is_none_or(|point| point.timestamp < current.timestamp)
                {
                    crate::http_server::analysis::history::push(&mut history.points, &observed);
                }
                history.sample = current;
                history.observation = observed.clone();
                value["history"] = serde_json::to_value(&history.points)?;
                value["observation"] = serde_json::to_value(observed)?;
            } else {
                let threads_only = value["threads_only"] == true;
                *value = if threads_only {
                    serde_json::to_value(observed.threads)?
                } else {
                    serde_json::to_value(observed)?
                };
            }
        }
        if value.get("raw_fd_owners").is_some() || value.get("raw_peer_keys").is_some() {
            let mut descriptors: crate::http_server::analysis::descriptors::FileDescriptors =
                serde_json::from_value(value.take())?;
            crate::http_server::analysis::descriptors::complete(&mut descriptors);
            *value = serde_json::to_value(descriptors)?;
            value.as_object_mut().unwrap().remove("raw_fd_owners");
            value.as_object_mut().unwrap().remove("raw_peer_keys");
            for entry in value["entries"].as_array_mut().unwrap() {
                for field in ["key", "mode", "socket_device"] {
                    entry.as_object_mut().unwrap().remove(field);
                }
            }
        }
        if !snapshot_processed
            && value.get("processes").is_some()
            && value.get("fd_relations").is_some()
        {
            *value = self.snapshot(serde_json::from_value(value.take())?)?;
        }
        let contexts: Vec<MappingSnapshot> = value
            .as_object_mut()
            .and_then(|value| value.remove("mapping_snapshots"))
            .map(serde_json::from_value)
            .transpose()?
            .unwrap_or_default();
        let contexts: HashMap<_, _> = contexts
            .into_iter()
            .map(|context| (context.epoch, context.maps))
            .collect();
        if value
            .get("live_samples")
            .and_then(Value::as_array)
            .is_none()
        {
            return Ok(());
        }
        let id: ProcessId = serde_json::from_value(value["summary"]["identity"].clone())?;
        let epoch = value["maps_captured_at"].as_u64().unwrap_or_default();
        let mut maps: Vec<MemoryMap> = contexts.values().flatten().cloned().collect();
        maps.sort_by_key(|map| map.start);
        maps.dedup_by_key(|map| (map.start, map.inode));
        if self.loaded.len() > 65536 {
            self.loaded.clear();
        }
        for map in maps.iter().filter(|map| map.executable && map.inode != 0) {
            if !self.loaded.insert((id, epoch, map.start, map.inode)) {
                continue;
            }
            if let Ok(Some(file)) =
                super::elf_reader::acquire(client, id, map.start, self.symbols.identity(map)).await
            {
                self.symbols.insert_image(map, &file);
            }
        }
        if self.unwinders.len() >= 64 && !self.unwinders.contains_key(&id) {
            self.unwinders.clear();
        }
        let unwinder = self.unwinders.entry(id).or_default();
        for sample in value["live_samples"].as_array_mut().unwrap() {
            let raw = sample.as_object_mut().unwrap();
            if let Some(time) = raw.remove("sample_time_ns").and_then(|time| time.as_u64()) {
                raw.insert(
                    "sample_age_ms".into(),
                    serde_json::json!(
                        crate::http_server::model::process::monotonic_ns().saturating_sub(time)
                            / 1_000_000
                    ),
                );
            }
            let registers = raw.remove("raw_registers").unwrap_or(Value::Null);
            let stack = raw.remove("stack_bytes").unwrap_or(Value::Null);
            let epoch = raw.remove("map_epoch").and_then(|epoch| epoch.as_u64());
            if registers.is_null() {
                continue;
            }
            if raw.get("architecture").and_then(Value::as_str) != Some(std::env::consts::ARCH) {
                let message = "Only native 64-bit process snapshots are supported";
                let error = raw
                    .get("error")
                    .and_then(Value::as_str)
                    .map_or_else(|| message.to_owned(), |error| format!("{error}; {message}"));
                raw.insert("error".into(), Value::String(error));
                raw.insert("registers".into(), serde_json::json!([]));
                raw.insert("call_stack".into(), serde_json::json!([]));
                raw.insert("unwind_stop".into(), Value::String(message.into()));
                raw.insert("disassembly".into(), Value::Null);
                continue;
            }
            let registers: RegisterSet = serde_json::from_value(registers)?;
            let stack: Vec<u8> = serde_json::from_value(stack)?;
            let maps = epoch
                .and_then(|epoch| contexts.get(&epoch))
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            unwinder.refresh(maps, &self.symbols);
            let (mut frames, reason) = unwinder.walk(&registers, maps, &stack);
            for (index, frame) in frames.iter_mut().enumerate() {
                let address = instruction_address(frame.address, index > 0);
                let info = maps
                    .iter()
                    .find(|map| map.contains(address) && map.inode != 0)
                    .and_then(|map| {
                        let elf = self.symbols.get(map)?;
                        resolve_frame(
                            elf_address(address, map, &elf, unsafe {
                                libc::sysconf(libc::_SC_PAGESIZE) as u64
                            })?,
                            &elf,
                        )
                    })
                    .unwrap_or_default();
                frame.symbol = info.name;
                frame.symbol_offset = info.offset.map(|offset| format!("0x{offset:x}"));
                frame.source_file = info.file;
                frame.line = info.line;
                frame.inline_frames = info.inline_frames;
            }
            raw.insert(
                "registers".into(),
                serde_json::to_value(from_sample(&registers, maps))?,
            );
            raw.insert("call_stack".into(), serde_json::to_value(frames)?);
            raw.insert("unwind_stop".into(), Value::String(reason));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn observation(ticks: u64, time: u64) -> Value {
        json!({"summary":{"identity":{"pid":42,"start_time_ticks":10}},"live_samples":[{"raw_registers":null,"stack_bytes":[],"map_epoch":null,"disassembly":null}],"process_sample":{
            "timestamp":time/1_000_000,"process_id":{"pid":42,"start_time_ticks":10},"rss_bytes":1024,"vms_bytes":2048,"minor_faults":0,"major_faults":0,"voluntary_context_switches":null,"nonvoluntary_context_switches":null,"io":null,"threads":[],"cpu":0,"nice":0,"priority":20,"ticks":ticks,"measured_at":time
        }})
    }
    #[tokio::test]
    async fn retained_samples_use_their_captured_mapping_epoch() {
        use crate::http_server::model::memory::parse_map;
        let mut processor = ObservationProcessor::default();
        let mut registers = RegisterSet::default();
        registers.values[0] = 0x1008;
        registers.values[RegisterSet::PC] = 0x1008;
        registers.values[RegisterSet::SP] = 0x1000;
        let sample = |epoch| json!({"architecture":std::env::consts::ARCH,"raw_registers":registers,"stack_bytes":[],"map_epoch":epoch,"sample_time_ns":crate::http_server::model::process::monotonic_ns().saturating_sub(3_000_000_000)});
        let mut response = MonitorResponse::Data(json!({
            "summary":{"identity":{"pid":42,"start_time_ticks":10}},
            "mapping_snapshots":[
                MappingSnapshot {epoch:1,maps:vec![parse_map("1000-2000 r--p 0 00:00 0 /old").unwrap()]},
                MappingSnapshot {epoch:2,maps:vec![parse_map("1000-2000 r--p 0 00:00 0 /new").unwrap()]}
            ],
            "live_samples":[sample(1),sample(2)]
        }));
        processor
            .process(&MonitorClient::unavailable(), None, &mut response)
            .await
            .unwrap();
        let MonitorResponse::Data(value) = response else {
            panic!()
        };
        for (index, path) in [(0, "/old"), (1, "/new")] {
            let sample = &value["live_samples"][index];
            let rax = sample["registers"]
                .as_array()
                .unwrap()
                .iter()
                .find(|register| {
                    register["name"]
                        == if cfg!(target_arch = "aarch64") {
                            "X0"
                        } else {
                            "RAX"
                        }
                })
                .unwrap();
            assert_eq!(rax["mapping"]["pathname"], path);
            assert!(sample.get("map_epoch").is_none());
            assert!(sample.get("sample_time_ns").is_none());
            assert!(sample["sample_age_ms"].as_u64().unwrap() >= 3000);
        }
        assert!(value.get("mapping_snapshots").is_none());
    }

    #[tokio::test]
    async fn process_histories_are_independent_and_repeated_samples_preserve_rates() {
        let mut processor = ObservationProcessor::default();
        let client = MonitorClient::unavailable();
        let mut first = MonitorResponse::Started {
            session: 1,
            initial: MonitorEvent::Observation(observation(100, 1_000_000_000)),
        };
        processor.process(&client, None, &mut first).await.unwrap();
        let mut next = MonitorResponse::Event {
            event: Some(MonitorEvent::Observation(observation(150, 2_000_000_000))),
            closed: false,
        };
        processor
            .process(&client, Some(1), &mut next)
            .await
            .unwrap();
        let MonitorResponse::Event {
            event: Some(MonitorEvent::Observation(value)),
            ..
        } = next
        else {
            panic!()
        };
        assert!(value["observation"]["cpu_percent"].as_f64().unwrap() > 0.0);
        assert_eq!(value["history"].as_array().unwrap().len(), 2);
        assert!(value.get("process_sample").is_none());
        assert!(value["live_samples"][0].get("raw_registers").is_none());
        let mut repeated = MonitorResponse::Event {
            event: Some(MonitorEvent::Observation(observation(150, 2_000_000_000))),
            closed: false,
        };
        processor
            .process(&client, Some(1), &mut repeated)
            .await
            .unwrap();
        let MonitorResponse::Event {
            event: Some(MonitorEvent::Observation(repeated)),
            ..
        } = repeated
        else {
            panic!()
        };
        assert_eq!(
            repeated["observation"]["cpu_percent"],
            value["observation"]["cpu_percent"]
        );
        assert_eq!(repeated["history"].as_array().unwrap().len(), 2);
        let mut second = MonitorResponse::Started {
            session: 2,
            initial: MonitorEvent::Observation(observation(150, 2_000_000_000)),
        };
        processor.process(&client, None, &mut second).await.unwrap();
        let MonitorResponse::Started {
            initial: MonitorEvent::Observation(value),
            ..
        } = second
        else {
            panic!()
        };
        assert!(value["observation"]["cpu_percent"].is_null());
        assert_eq!(value["history"].as_array().unwrap().len(), 1);
        processor.close(1);
        assert_eq!(processor.histories.len(), 1);
    }
    #[tokio::test]
    async fn unsupported_abis_retain_acquisition_errors_and_discard_private_samples() {
        for architecture in [
            Value::Null,
            json!(if cfg!(target_arch = "aarch64") {
                "x86_64"
            } else {
                "aarch64"
            }),
        ] {
            let mut response = MonitorResponse::Data(json!({
                "summary":{"identity":{"pid":42,"start_time_ticks":10}},
                "live_samples":[{"architecture":architecture,"raw_registers":RegisterSet::default(),
                    "stack_bytes":[1,2,3],"map_epoch":1,"error":"Partial capture",
                    "registers":[],"call_stack":[],"disassembly":{"address":"0x1000","bytes":[144],"error":null}}]
            }));
            ObservationProcessor::default()
                .process(&MonitorClient::unavailable(), None, &mut response)
                .await
                .unwrap();
            let MonitorResponse::Data(value) = response else {
                panic!()
            };
            let sample = &value["live_samples"][0];
            assert!(
                sample["error"]
                    .as_str()
                    .unwrap()
                    .starts_with("Partial capture; Only native")
            );
            assert!(sample["disassembly"].is_null());
            assert!(sample["registers"].as_array().unwrap().is_empty());
            for field in ["raw_registers", "stack_bytes", "map_epoch"] {
                assert!(sample.get(field).is_none());
            }
        }
    }
}
