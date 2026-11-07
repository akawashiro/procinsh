use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};

/// Application-side full/delta selection over complete structured observations.
#[derive(Default)]
pub(super) struct SnapshotEncoder {
    previous: Option<Arc<Value>>,
    last_full: Option<Instant>,
    epochs: HashMap<String, Value>,
    sequence: u64,
}

fn field_identity_key(value: &Value, field: &str) -> String {
    value[field].to_string()
}

fn build_array_delta(old: &[Value], current: &[Value], field: &str) -> Option<Value> {
    let previous: HashMap<_, _> = old
        .iter()
        .map(|v| (field_identity_key(v, field), v))
        .collect();
    let live: std::collections::HashSet<_> = current
        .iter()
        .map(|v| field_identity_key(v, field))
        .collect();
    let upsert: Vec<_> = current
        .iter()
        .filter(|v| {
            previous
                .get(&field_identity_key(v, field))
                .is_none_or(|p| *p != *v)
        })
        .collect();
    let remove: Vec<_> = old
        .iter()
        .filter(|v| !live.contains(&field_identity_key(v, field)))
        .map(|v| &v[field])
        .collect();
    let delta = json!({"upsert":upsert,"remove":remove});
    (delta.to_string().len() + 6 < serde_json::to_vec(current).unwrap().len()).then_some(delta)
}

fn snapshot_array_entries(value: &Value) -> &[Value] {
    value.as_array().map(Vec::as_slice).unwrap_or_default()
}

impl SnapshotEncoder {
    pub(super) fn encode(
        &mut self,
        snapshot: Arc<Value>,
        force_full: bool,
        now: Instant,
    ) -> serde_json::Result<String> {
        let full = force_full
            || self
                .last_full
                .is_none_or(|last| now.duration_since(last) >= Duration::from_secs(60));
        let mut output = (*snapshot).clone();
        let old = self.previous.as_deref().unwrap_or(&Value::Null);
        let previous: HashMap<_, _> = snapshot_array_entries(&old["processes"])
            .iter()
            .map(|p| (field_identity_key(p, "identity"), p))
            .collect();
        let mut epochs = HashMap::new();
        if let Some(processes) = output["processes"].as_array_mut() {
            for p in processes {
                let id = field_identity_key(p, "identity");
                let before = previous.get(&id);
                let unchanged = !full && before.is_some_and(|old| old["maps"] == p["maps"]);
                if unchanged {
                    p["maps_epoch"] = self.epochs[&id].clone();
                    p.as_object_mut().unwrap().remove("maps");
                } else if !full
                    && let Some(before) = before
                    && let Some(change) = build_array_delta(
                        snapshot_array_entries(&before["maps"]),
                        snapshot_array_entries(&p["maps"]),
                        "start",
                    )
                {
                    p.as_object_mut().unwrap().remove("maps");
                    p["maps_delta"] = change;
                }
                epochs.insert(id, p["maps_epoch"].clone());
            }
        }
        // RDMA ports are small and always sent whole.
        for field in ["fd_relations", "rdma_relations", "device_handles"] {
            if !full
                && let Some(change) = build_array_delta(
                    snapshot_array_entries(&old[field]),
                    snapshot_array_entries(&output[field]),
                    "id",
                )
            {
                output.as_object_mut().unwrap().remove(field);
                output[format!("{field}_delta")] = change;
            }
        }
        self.sequence += 1;
        output["kind"] = json!(if full { "full" } else { "delta" });
        output["sequence"] = json!(self.sequence);
        if !full {
            output["base_sequence"] = json!(self.sequence - 1);
        }
        let data = serde_json::to_string(&output)?;
        log::debug!(
            "snapshot_delta kind={} payload_bytes={}",
            if full { "full" } else { "delta" },
            data.len()
        );
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
    use super::*;
    #[test]
    fn delta_baseline_refresh_and_pid_reuse() {
        let now = Instant::now();
        let sample = |start, epoch| {
            Arc::new(
                json!({"processes":[{"identity":{"pid":42,"start_time_ticks":start},"maps":[{"start":"0x1000"}],"maps_epoch":epoch}], "fd_relations":[],"device_handles":[]}),
            )
        };
        let mut encoder = SnapshotEncoder::default();
        let parse = |s: String| serde_json::from_str::<Value>(&s).unwrap();
        assert_eq!(
            parse(encoder.encode(sample(1, 1), false, now).unwrap())["kind"],
            "full"
        );
        let next = parse(encoder.encode(sample(1, 2), false, now).unwrap());
        assert!(next["processes"][0].get("maps").is_none());
        assert_eq!(next["processes"][0]["maps_epoch"], 1);
        assert!(
            parse(encoder.encode(sample(2, 3), false, now).unwrap())["processes"][0]["maps"]
                .is_array()
        );
        assert_eq!(
            parse(
                encoder
                    .encode(sample(2, 4), false, now + Duration::from_secs(60))
                    .unwrap()
            )["kind"],
            "full"
        );
        assert_eq!(
            parse(
                encoder
                    .encode(sample(2, 5), true, now + Duration::from_secs(61))
                    .unwrap()
            )["kind"],
            "full"
        );
    }

    #[test]
    fn rdma_relations_use_id_deltas_and_ports_are_sent_whole() {
        let now = Instant::now();
        let relation = |qpn: u32, label: &str| json!({"id": format!("rdma:ib0/1:{qpn}"), "label": label, "padding": "x".repeat(40)});
        let snapshot = |relations: Vec<Value>| {
            Arc::new(
                json!({"processes": [], "fd_relations": [], "device_handles": [],
                "rdma_ports": [{"device": "ib0", "port": 1}], "rdma_relations": relations}),
            )
        };
        let initial: Vec<_> = (0..20).map(|q| relation(q, "a")).collect();
        let mut encoder = SnapshotEncoder::default();
        let parse = |s: String| serde_json::from_str::<Value>(&s).unwrap();
        let full = parse(
            encoder
                .encode(snapshot(initial.clone()), false, now)
                .unwrap(),
        );
        assert_eq!(full["rdma_relations"].as_array().unwrap().len(), 20);
        let mut current = initial.clone();
        current.remove(3);
        current[0] = relation(0, "changed");
        let delta = parse(encoder.encode(snapshot(current), false, now).unwrap());
        assert!(delta.get("rdma_relations").is_none());
        assert_eq!(
            delta["rdma_relations_delta"],
            json!({"upsert": [relation(0, "changed")], "remove": ["rdma:ib0/1:3"]})
        );
        assert_eq!(delta["rdma_ports"], full["rdma_ports"]);
    }
}
