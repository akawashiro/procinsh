//! Regular-file activity, independent of IPC/CPU and IBS availability.
use crate::process::ProcessId;
use anyhow::{Context, Result};
use libbpf_rs::{MapCore, MapFlags, ObjectBuilder, RingBufferBuilder};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

pub const COVERAGE: &str = "Regular-file read/write, pread/pwrite and vectored I/O, including page cache. Not physical disk traffic. mmap, io_uring, splice/sendfile and kernel workers are not observed.";
const MAX_AGGREGATES: usize = 8192;
#[derive(Clone, Debug, Serialize)]
pub struct FileActivity {
    process_id: ProcessId,
    resource: String,
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
                * crate::process::procfs::ticks_per_second() as u128)
                / 1_000_000_000) as u64,
        },
        resource: format!(
            "file:{}:{}:{}:{}",
            dev >> 20,
            dev & ((1 << 20) - 1),
            u64_at(8),
            u32_at(44)
        ),
        path,
        write: u32_at(36) != 0,
        bytes,
        count: 1,
    })
}
#[derive(Default)]
struct Batch {
    events: HashMap<(ProcessId, String, bool), FileActivity>,
    dropped: u64,
}
impl Batch {
    fn add(&mut self, event: FileActivity) {
        let key = (event.process_id, event.resource.clone(), event.write);
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
pub struct Files {
    ring: libbpf_rs::RingBuffer<'static>,
    _links: Vec<libbpf_rs::Link>,
    obj: libbpf_rs::Object,
    batch: Arc<Mutex<Batch>>,
}
impl Files {
    pub fn new() -> Result<Self> {
        let obj = ObjectBuilder::default()
            .open_memory(include_bytes!(concat!(env!("OUT_DIR"), "/files.bpf.o")))?
            .load()
            .context("File I/O requires CAP_BPF / CAP_PERFMON and compatible VFS BTF hooks")?;
        let mut links = Vec::new();
        for prog in obj.progs_mut() {
            links.push(
                prog.attach()
                    .with_context(|| format!("attach {:?}", prog.name()))?,
            );
        }
        let batch = Arc::new(Mutex::new(Batch::default()));
        let shared = batch.clone();
        let map = obj
            .maps()
            .find(|m| m.name() == "events")
            .context("file events map")?;
        let mut builder = RingBufferBuilder::new();
        builder.add(&map, move |data| {
            let mut batch = shared.lock().unwrap();
            if let Some(event) = decode(data) {
                batch.add(event);
            } else {
                batch.dropped += 1;
            }
            0
        })?;
        Ok(Self {
            ring: builder.build()?,
            _links: links,
            obj,
            batch,
        })
    }
    pub fn poll(&self) -> Result<()> {
        let result = self.ring.consume_raw_n(8192);
        if result < 0 {
            return Err(std::io::Error::from_raw_os_error(-result).into());
        }
        Ok(())
    }
    pub fn drain(&self) -> Vec<FileActivity> {
        self.batch
            .lock()
            .unwrap()
            .events
            .drain()
            .map(|(_, event)| event)
            .collect()
    }
    pub fn lost(&self) -> u64 {
        self.obj
            .maps()
            .find(|m| m.name() == "lost")
            .and_then(|m| m.lookup(&0u32.to_ne_bytes(), MapFlags::ANY).ok().flatten())
            .and_then(|bytes| bytes.try_into().ok())
            .map(u64::from_ne_bytes)
            .unwrap_or(0)
            + self.batch.lock().unwrap().dropped
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
    fn decoding_and_path_failure() {
        let mut raw = event();
        let e = decode(&raw).unwrap();
        assert_eq!(e.path.as_deref(), Some("/tmp/x"));
        assert_eq!(e.resource, "file:8:1:42:0");
        assert_eq!(
            e.process_id.start_time_ticks,
            crate::process::procfs::ticks_per_second() as u64
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
        assert_eq!(
            batch.events[&(e.process_id, e.resource.clone(), false)].bytes,
            14
        );
        for i in 0..MAX_AGGREGATES {
            let mut next = e.clone();
            next.resource = i.to_string();
            batch.add(next);
        }
        assert_eq!(batch.events.len(), MAX_AGGREGATES);
        assert_eq!(batch.dropped, 3);
        batch.add(e.clone());
        assert_eq!(batch.events[&(e.process_id, e.resource, false)].count, 3);
    }
}
