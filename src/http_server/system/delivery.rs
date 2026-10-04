use super::snapshot::{FdRelation, SystemSnapshot};
use crate::http_server::process::{MemoryMap, ProcessId};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

/// Per-connection baseline of the last successfully serialized snapshot.
#[derive(Default)]
pub(in crate::http_server) struct SnapshotDelivery {
    previous: Option<Arc<SystemSnapshot>>,
    last_full: Option<Instant>,
    epochs: HashMap<ProcessId, u64>,
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
}
#[derive(Serialize)]
struct SnapshotPayload<'a> {
    captured_at: u64,
    processes: Vec<ProcessPayload<'a>>,
    fd_relations: &'a [FdRelation],
    warnings: &'a [String],
    inspected_processes: usize,
    inspected_fds: usize,
}
impl SnapshotDelivery {
    pub(in crate::http_server) fn encode(
        &mut self,
        snapshot: Arc<SystemSnapshot>,
        force_full: bool,
        now: Instant,
    ) -> serde_json::Result<String> {
        let full = force_full
            || self
                .last_full
                .is_none_or(|last| now.duration_since(last) >= Duration::from_secs(10));
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
                    maps: (!unchanged).then_some(p.maps.as_slice()),
                }
            })
            .collect();
        let data = serde_json::to_string(&SnapshotPayload {
            captured_at: snapshot.captured_at,
            processes,
            fd_relations: &snapshot.fd_relations,
            warnings: &snapshot.warnings,
            inspected_processes: snapshot.inspected_processes,
            inspected_fds: snapshot.inspected_fds,
        })?;
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
    use super::super::snapshot::Process;
    use super::*;
    use crate::http_server::{process::MemoryMap, resource::DeviceId};

    fn fixture(start: u64, mapped: bool, epoch: u64) -> Arc<SystemSnapshot> {
        Arc::new(SystemSnapshot {
            processes: vec![Process {
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
        delivery: &mut SnapshotDelivery,
        data: Arc<SystemSnapshot>,
        full: bool,
        now: Instant,
    ) -> serde_json::Value {
        serde_json::from_str(&delivery.encode(data, full, now).unwrap()).unwrap()
    }
    #[test]
    fn unchanged_maps_are_omitted_until_refresh_or_gap() {
        let now = Instant::now();
        let mut delivery = SnapshotDelivery::default();
        let first = payload(&mut delivery, fixture(1, true, 10), false, now);
        let map = &first["processes"][0]["maps"][0];
        assert_eq!(map["start"], "0x0000000000001000");
        assert!(map.get("rss_bytes").is_none() && map.get("pss_bytes").is_none());
        for second in [1, 2, 9] {
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
            now + Duration::from_secs(10),
        );
        assert!(full["processes"][0]["maps"].is_array());
        let gap = payload(
            &mut delivery,
            fixture(1, true, 40),
            true,
            now + Duration::from_secs(11),
        );
        assert_eq!(gap["processes"][0]["maps_epoch"], 40);
        let next = payload(
            &mut delivery,
            fixture(1, true, 50),
            false,
            now + Duration::from_secs(20),
        );
        assert!(next["processes"][0].get("maps").is_none());
    }
    #[test]
    fn changed_empty_reused_and_new_connections_send_maps() {
        let now = Instant::now();
        let mut delivery = SnapshotDelivery::default();
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
            &mut SnapshotDelivery::default(),
            fixture(2, true, 5),
            false,
            now,
        );
        assert!(other["processes"][0]["maps"].is_array());
    }
}
