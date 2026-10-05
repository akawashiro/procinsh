use super::snapshot::{FdRelation, SystemSnapshot};
use crate::http_server::process::{MemoryMap, ProcessId};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

/// Encodes full/delta snapshots using a per-connection baseline of the last
/// successfully serialized snapshot.
#[derive(Default)]
pub(in crate::http_server) struct SnapshotEncoder {
    previous: Option<Arc<SystemSnapshot>>,
    last_full: Option<Instant>,
    epochs: HashMap<ProcessId, u64>,
    sequence: u64,
}
#[derive(Serialize)]
struct ProcessPayload<'a> {
    identity: &'a ProcessId,
    parent_id: &'a Option<ProcessId>,
    name: &'a String,
    uid: &'a Option<u32>,
    username: &'a Option<String>,
    euid: &'a Option<u32>,
    effective_username: &'a Option<String>,
    maps_epoch: u64,
    maps_error: &'a Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    maps: Option<&'a [MemoryMap]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    maps_delta: Option<EntryDelta<'a, MemoryMap, String>>,
}
#[derive(Serialize)]
struct SnapshotPayload<'a> {
    captured_at: u64,
    processes: Vec<ProcessPayload<'a>>,
    kind: &'static str,
    sequence: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_sequence: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fd_relations: Option<&'a [FdRelation]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    fd_relations_delta: Option<EntryDelta<'a, FdRelation, String>>,
    warnings: &'a [String],
    inspected_processes: usize,
    inspected_fds: usize,
}
#[derive(Serialize)]
struct EntryDelta<'a, T: Serialize, K: Serialize> {
    upsert: Vec<&'a T>,
    remove: Vec<K>,
}
fn map_delta<'a>(old: &[MemoryMap], current: &'a [MemoryMap]) -> EntryDelta<'a, MemoryMap, String> {
    let previous: HashMap<_, _> = old.iter().map(|m| (m.start, m)).collect();
    let live: std::collections::HashSet<_> = current.iter().map(|m| m.start).collect();
    EntryDelta {
        upsert: current
            .iter()
            .filter(|m| previous.get(&m.start).is_none_or(|p| *p != *m))
            .collect(),
        remove: old
            .iter()
            .filter(|m| !live.contains(&m.start))
            .map(|m| format!("0x{:016x}", m.start))
            .collect(),
    }
}
impl SnapshotEncoder {
    /// Encodes full or delta SSE snapshots with a sequence and, for deltas, base sequence.
    /// Map deltas use start addresses; FD relation deltas use relation IDs. Collections
    /// use replacement when a delta is larger. Unchanged maps are omitted and retain
    /// their epoch. Initial, forced gap recovery and 60-second refresh snapshots
    /// include all maps. The baseline advances only after successful serialization.
    pub(in crate::http_server) fn encode(
        &mut self,
        snapshot: Arc<SystemSnapshot>,
        force_full: bool,
        now: Instant,
    ) -> serde_json::Result<String> {
        let started = Instant::now();
        let sequence = self.sequence + 1;
        let full = force_full
            || self
                .last_full
                .is_none_or(|last| now.duration_since(last) >= Duration::from_secs(60));
        let previous: HashMap<_, _> = self
            .previous
            .as_ref()
            .into_iter()
            .flat_map(|s| &s.processes)
            .map(|p| (p.identity, p))
            .collect();
        let mut epochs = HashMap::new();
        let processes = snapshot
            .processes
            .iter()
            .map(|p| {
                let unchanged = !full
                    && previous
                        .get(&p.identity)
                        .is_some_and(|old| old.maps == p.maps);
                let epoch = if unchanged {
                    self.epochs[&p.identity]
                } else {
                    p.maps_epoch
                };
                epochs.insert(p.identity, epoch);
                let delta = if !full && !unchanged {
                    previous
                        .get(&p.identity)
                        .map(|old| map_delta(&old.maps, &p.maps))
                } else {
                    None
                };
                let delta = delta.filter(|d| {
                    serde_json::to_vec(d).unwrap().len() + 6
                        < serde_json::to_vec(&p.maps).unwrap().len()
                });
                let include_maps = !unchanged && delta.is_none();
                ProcessPayload {
                    identity: &p.identity,
                    parent_id: &p.parent_id,
                    name: &p.name,
                    uid: &p.uid,
                    username: &p.username,
                    euid: &p.euid,
                    effective_username: &p.effective_username,
                    maps_error: &p.maps_error,
                    maps_epoch: epoch,
                    maps: include_maps.then_some(p.maps.as_slice()),
                    maps_delta: delta,
                }
            })
            .collect();
        let old_relations = self
            .previous
            .as_ref()
            .map(|s| s.fd_relations.as_slice())
            .unwrap_or_default();
        let old: HashMap<_, _> = old_relations.iter().map(|r| (&r.id, r)).collect();
        let live: std::collections::HashSet<_> =
            snapshot.fd_relations.iter().map(|r| &r.id).collect();
        let relations_delta = (!full)
            .then(|| EntryDelta {
                upsert: snapshot
                    .fd_relations
                    .iter()
                    .filter(|r| old.get(&r.id).is_none_or(|p| *p != *r))
                    .collect(),
                remove: old_relations
                    .iter()
                    .filter(|r| !live.contains(&r.id))
                    .map(|r| r.id.clone())
                    .collect(),
            })
            .filter(|d| {
                serde_json::to_vec(d).unwrap().len() + 6
                    < serde_json::to_vec(&snapshot.fd_relations).unwrap().len()
            });
        let data = serde_json::to_string(&SnapshotPayload {
            captured_at: snapshot.captured_at,
            processes,
            kind: if full { "full" } else { "delta" },
            sequence,
            base_sequence: (!full).then_some(self.sequence),
            fd_relations: relations_delta
                .is_none()
                .then_some(snapshot.fd_relations.as_slice()),
            fd_relations_delta: relations_delta,
            warnings: &snapshot.warnings,
            inspected_processes: snapshot.inspected_processes,
            inspected_fds: snapshot.inspected_fds,
        })?;
        log::debug!(
            "snapshot_delta kind={} payload_bytes={} encode_us={}",
            if full { "full" } else { "delta" },
            data.len(),
            started.elapsed().as_micros()
        );
        self.sequence = sequence;
        self.previous = Some(snapshot);
        self.epochs = epochs;
        if full {
            self.last_full = Some(now);
        }
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::super::snapshot::ProcessSnapshot;
    use super::*;
    use crate::http_server::{process::MemoryMap, resource::DeviceId};

    fn fixture(start: u64, mapped: bool, epoch: u64) -> Arc<SystemSnapshot> {
        Arc::new(SystemSnapshot {
            processes: vec![ProcessSnapshot {
                identity: ProcessId {
                    pid: 42,
                    start_time_ticks: start,
                },
                parent_id: None,
                name: "test".into(),
                uid: None,
                username: None,
                euid: None,
                effective_username: None,
                maps_epoch: epoch,
                maps_error: None,
                maps: if mapped {
                    vec![MemoryMap {
                        start: 4096,
                        end: 8192,
                        readable: true,
                        writable: false,
                        executable: false,
                        private: true,
                        file_offset: 0,
                        device: DeviceId::from_stat(0),
                        inode: 1,
                        pathname: None,
                    }]
                } else {
                    vec![]
                },
            }],
            ..Default::default()
        })
    }
    fn payload(
        delivery: &mut SnapshotEncoder,
        data: Arc<SystemSnapshot>,
        full: bool,
        now: Instant,
    ) -> serde_json::Value {
        serde_json::from_str(&delivery.encode(data, full, now).unwrap()).unwrap()
    }
    #[test]
    fn unchanged_maps_are_omitted_until_refresh_or_gap() {
        let now = Instant::now();
        let mut delivery = SnapshotEncoder::default();
        let first = payload(&mut delivery, fixture(1, true, 10), false, now);
        let map = &first["processes"][0]["maps"][0];
        assert_eq!(map["start"], "0x0000000000001000");
        assert!(map.get("rss_bytes").is_none() && map.get("pss_bytes").is_none());
        for second in [1, 10, 59] {
            let next = payload(
                &mut delivery,
                fixture(1, true, 20),
                false,
                now + Duration::from_secs(second),
            );
            assert!(next["processes"][0].get("maps").is_none());
            assert_eq!(next["processes"][0]["maps_epoch"], 10);
        }
        let full = payload(
            &mut delivery,
            fixture(1, true, 30),
            false,
            now + Duration::from_secs(60),
        );
        assert!(full["processes"][0]["maps"].is_array());
        let gap = payload(
            &mut delivery,
            fixture(1, true, 40),
            true,
            now + Duration::from_secs(61),
        );
        assert_eq!(gap["processes"][0]["maps_epoch"], 40);
        let next = payload(
            &mut delivery,
            fixture(1, true, 50),
            false,
            now + Duration::from_secs(120),
        );
        assert!(next["processes"][0].get("maps").is_none());
        let refresh = payload(
            &mut delivery,
            fixture(1, true, 60),
            false,
            now + Duration::from_secs(121),
        );
        assert_eq!(refresh["kind"], "full");
        assert!(refresh["processes"][0]["maps"].is_array());
    }
    #[test]
    fn changed_empty_reused_and_new_connections_send_maps() {
        let now = Instant::now();
        let mut delivery = SnapshotEncoder::default();
        payload(&mut delivery, fixture(1, true, 1), false, now);
        let empty = payload(&mut delivery, fixture(1, false, 2), false, now);
        assert_eq!(empty["processes"][0]["maps"], serde_json::json!([]));
        let changed = payload(&mut delivery, fixture(1, true, 3), false, now);
        assert!(changed["processes"][0]["maps"].is_array());
        let reused = payload(&mut delivery, fixture(2, true, 4), false, now);
        assert!(reused["processes"][0]["maps"].is_array());
        payload(
            &mut delivery,
            Arc::new(SystemSnapshot::default()),
            false,
            now,
        );
        let returned = payload(&mut delivery, fixture(2, true, 5), false, now);
        assert!(returned["processes"][0]["maps"].is_array());
        let other = payload(
            &mut SnapshotEncoder::default(),
            fixture(2, true, 5),
            false,
            now,
        );
        assert!(other["processes"][0]["maps"].is_array());
    }
    #[test]
    fn entry_deltas_reconstruct_maps_and_relations() {
        let now = Instant::now();
        let mut initial = (*fixture(1, true, 1)).clone();
        initial.processes[0].maps = (0..100)
            .map(|i| {
                let mut m = initial.processes[0].maps[0].clone();
                m.start = 4096 + i * 8192;
                m.end = m.start + 4096;
                m
            })
            .collect();
        initial.fd_relations = (0..100)
            .map(|i| FdRelation {
                id: format!("edge-{i}"),
                endpoint: super::super::snapshot::FdEndpoint {
                    process_id: initial.processes[0].identity,
                    fd: i,
                    fd_count: 1,
                    resource: crate::http_server::resource::IpcIdentity {
                        kind: crate::http_server::resource::IpcKind::Pipe,
                        device: DeviceId::from_stat(0),
                        inode: 1,
                    },
                    kind: crate::http_server::socket_types::FdKind::Pipe,
                    access: crate::http_server::socket_types::FdAccess::Read,
                },
                peer: None,
                label: "pipe".into(),
                socket: None,
                candidate: false,
                shared: false,
            })
            .collect();
        let mut delivery = SnapshotEncoder::default();
        let full = payload(&mut delivery, Arc::new(initial.clone()), false, now);
        assert_eq!(full["kind"], "full");
        let mut current = initial.clone();
        current.processes[0].maps[1].writable = true;
        current.processes[0].maps.remove(2);
        let mut added_map = current.processes[0].maps[0].clone();
        added_map.start = 1_000_000;
        added_map.end = 1_004_096;
        current.processes[0].maps.push(added_map);
        current.processes[0].maps_epoch = 2;
        current.fd_relations[0].label = "changed".into();
        current.fd_relations.remove(1);
        let mut added_relation = current.fd_relations[0].clone();
        added_relation.id = "new-edge".into();
        current.fd_relations.push(added_relation);
        let delta = payload(&mut delivery, Arc::new(current.clone()), false, now);
        assert_eq!(delta["base_sequence"], full["sequence"]);
        assert!(delta["processes"][0].get("maps").is_none());
        assert!(delta.get("fd_relations").is_none());
        fn apply(
            old: &serde_json::Value,
            delta: &serde_json::Value,
            key: &str,
        ) -> serde_json::Value {
            let mut entries: std::collections::BTreeMap<String, serde_json::Value> = old
                .as_array()
                .unwrap()
                .iter()
                .map(|v| (v[key].as_str().unwrap().into(), v.clone()))
                .collect();
            for k in delta["remove"].as_array().unwrap() {
                entries.remove(k.as_str().unwrap());
            }
            for v in delta["upsert"].as_array().unwrap() {
                entries.insert(v[key].as_str().unwrap().into(), v.clone());
            }
            serde_json::json!(entries.into_values().collect::<Vec<_>>())
        }
        assert_eq!(
            apply(
                &full["processes"][0]["maps"],
                &delta["processes"][0]["maps_delta"],
                "start"
            ),
            serde_json::to_value(&current.processes[0].maps).unwrap()
        );
        let expected = serde_json::to_value(&current.fd_relations).unwrap();
        let actual = apply(&full["fd_relations"], &delta["fd_relations_delta"], "id");
        let canonical = apply(
            &expected,
            &serde_json::json!({"upsert":[],"remove":[]}),
            "id",
        );
        assert_eq!(actual, canonical);
        let forced = payload(&mut delivery, Arc::new(current), true, now);
        assert_eq!(forced["kind"], "full");
        assert!(forced.get("base_sequence").is_none());
    }
}
