//! Regular-file activity, independent of IPC/CPU availability.
//!
//! # Interface
//!
//! - [`COVERAGE`] (`pub(super) const COVERAGE: &str`): observation coverage.
//! - [`FileActivity`] (`pub(in crate::http_server) struct`): serialized observation.
//! - [`FileActivityCollector`] (`pub(super) struct`): owns and polls the sensor.
use crate::http_server::{
    process::ProcessId,
    resource::{DeviceId, FileIdentity},
};
use anyhow::{Context, Result};
use aya::maps::{Array, MapData, RingBuf};
use serde::Serialize;
use std::collections::HashMap;

pub(super) const COVERAGE: &str = "Regular-file read/write, pread/pwrite and vectored I/O, including page cache. Not physical disk traffic. mmap, io_uring, splice/sendfile and kernel workers are not observed.";
const MAX_AGGREGATES: usize = 8192;
#[derive(Clone, Debug, Serialize)]
pub(in crate::http_server) struct FileActivity {
    process_id: ProcessId,
    file: FileIdentity,
    path: Option<String>,
    write: bool,
    bytes: u64,
    count: u64,
}

fn decode(data: &[u8]) -> Option<FileActivity> {
    if data.len() != 4144 {
        return None;
    }
    let u64_at = |i| u64::from_ne_bytes(data[i..i + 8].try_into().unwrap());
    let u32_at = |i| u32::from_ne_bytes(data[i..i + 4].try_into().unwrap());
    let bytes = u64_at(24);
    if bytes == 0 || u32_at(36) > 1 {
        return None;
    }
    let len = u32_at(40) as usize;
    let path = if (2..=4096).contains(&len) && data[48 + len - 1] == 0 {
        Some(String::from_utf8_lossy(&data[48..48 + len - 1]).into_owned())
    } else {
        None
    };
    let dev = u64_at(16);
    Some(FileActivity {
        process_id: ProcessId {
            pid: u32_at(32) as i32,
            start_time_ticks: ((u64_at(0) as u128
                * crate::http_server::process::ticks_per_second() as u128)
                / 1_000_000_000) as u64,
        },
        file: FileIdentity {
            device: DeviceId::from_kernel(dev),
            inode: u64_at(8),
            generation: u32_at(44),
        },
        path,
        write: u32_at(36) != 0,
        bytes,
        count: 1,
    })
}
#[derive(Default)]
struct Batch {
    events: HashMap<(ProcessId, FileIdentity, bool), FileActivity>,
    dropped: u64,
}
impl Batch {
    fn add(&mut self, event: FileActivity) {
        let key = (event.process_id, event.file, event.write);
        if let Some(prior) = self.events.get_mut(&key) {
            prior.bytes = prior.bytes.saturating_add(event.bytes);
            prior.count = prior.count.saturating_add(event.count);
            if event.path.is_some() {
                prior.path = event.path;
            }
        } else if self.events.len() < MAX_AGGREGATES {
            self.events.insert(key, event);
        } else {
            self.dropped += 1;
        }
    }
}
/// Owns BPF resources and collects aggregated regular-file I/O activity.
pub(super) struct FileActivityCollector {
    ring: RingBuf<MapData>,
    lost: Array<MapData, u64>,
    _obj: aya::Ebpf,
    batch: Batch,
}
impl FileActivityCollector {
    pub(super) fn new() -> Result<Self> {
        let mut obj = super::bpf::load("files")?;
        let ring = RingBuf::try_from(obj.take_map("events").context("file events map")?)?;
        let lost = Array::try_from(obj.take_map("lost").context("file lost map")?)?;
        Ok(Self {
            ring,
            lost,
            _obj: obj,
            batch: Batch::default(),
        })
    }

    pub(super) fn poll(&mut self) -> Result<()> {
        for _ in 0..8192 {
            let Some(item) = self.ring.next() else {
                break;
            };
            if let Some(event) = decode(&item) {
                self.batch.add(event);
            } else {
                self.batch.dropped += 1;
            }
        }
        Ok(())
    }

    pub(super) fn drain(&mut self) -> Vec<FileActivity> {
        self.batch.events.drain().map(|(_, event)| event).collect()
    }

    pub(super) fn lost(&self) -> u64 {
        self.lost.get(&0, 0).unwrap_or(0) + self.batch.dropped
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    fn event() -> Vec<u8> {
        let mut data = vec![0; 4144];
        data[0..8].copy_from_slice(&1_000_000_000u64.to_ne_bytes());
        data[8..16].copy_from_slice(&42u64.to_ne_bytes());
        data[16..24].copy_from_slice(&((8u64 << 20) | 1).to_ne_bytes());
        data[24..32].copy_from_slice(&7u64.to_ne_bytes());
        data[32..36].copy_from_slice(&123u32.to_ne_bytes());
        data[40..44].copy_from_slice(&6u32.to_ne_bytes());
        data[48..54].copy_from_slice(b"/tmp/x");
        data[40..44].copy_from_slice(&7u32.to_ne_bytes());
        data
    }

    #[test]
    fn file_activity_preserves_wire_fields_and_null_path() {
        let mut activity = decode(&event()).unwrap();
        activity.path = None;
        assert_eq!(
            serde_json::to_value(&activity).unwrap(),
            serde_json::json!({
                "process_id": {"pid": 123, "start_time_ticks": crate::http_server::process::ticks_per_second() as u64},
                "file": {"device":{"major":8,"minor":1},"inode":"42","generation":0}, "path": null, "write": false, "bytes": 7, "count": 1
            })
        );
    }

    #[test]
    fn decoding_and_path_failure() {
        let mut raw = event();
        let e = decode(&raw).unwrap();
        assert_eq!(e.path.as_deref(), Some("/tmp/x"));
        assert_eq!(
            e.file,
            FileIdentity {
                device: DeviceId { major: 8, minor: 1 },
                inode: 42,
                generation: 0
            }
        );
        assert_eq!(
            e.process_id.start_time_ticks,
            crate::http_server::process::ticks_per_second() as u64
        );
        raw[40..44].copy_from_slice(&0u32.to_ne_bytes());
        assert!(decode(&raw).unwrap().path.is_none());
        raw[40..44].copy_from_slice(&4097u32.to_ne_bytes());
        assert!(decode(&raw).unwrap().path.is_none());
        assert!(decode(&raw[..100]).is_none());
        raw[24..32].fill(0);
        assert!(decode(&raw).is_none());
    }

    #[test]
    fn aggregation_separates_direction_and_process_lifetime_and_is_bounded() {
        let e = decode(&event()).unwrap();
        let mut batch = Batch::default();
        batch.add(e.clone());
        batch.add(e.clone());
        let mut write = e.clone();
        write.write = true;
        batch.add(write);
        let mut reused = e.clone();
        reused.process_id.start_time_ticks += 1;
        batch.add(reused);
        assert_eq!(batch.events.len(), 3);
        assert_eq!(batch.events[&(e.process_id, e.file, false)].bytes, 14);
        for i in 0..MAX_AGGREGATES {
            let mut next = e.clone();
            next.file.inode = i as u64 + 1000;
            batch.add(next);
        }
        assert_eq!(batch.events.len(), MAX_AGGREGATES);
        assert_eq!(batch.dropped, 3);
        batch.add(e.clone());
        assert_eq!(batch.events[&(e.process_id, e.file, false)].count, 3);
    }
}
