//! Regular-file activity, independent of IPC/CPU availability.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`COVERAGE`] | `pub(super)` | `const COVERAGE: &str` | //! | [`FileActivityCollector`] | `pub(super)` | `struct FileActivityCollector` | //! | [`create_file_activity_collector`] | `pub(super)` | `fn create_file_activity_collector() -> `[`Result`]`<`[`FileActivityCollector`]`>` | //! | [`poll_file_activity_collector`] | `pub(super)` | `fn poll_file_activity_collector(state: &`[`FileActivityCollector`]`) -> `[`Result`]`<()>` | //! | [`drain_file_activity_collector`] | `pub(super)` | `fn drain_file_activity_collector(state: &`[`FileActivityCollector`]`) -> `[`Vec`]`<`[`FileActivity`]`>` | //! | [`lost_file_activity_collector`] | `pub(super)` | `fn lost_file_activity_collector(state: &`[`FileActivityCollector`]`) -> u64` | //! //! [`FileActivityCollector`] (`pub(super)`): `struct FileActivityCollector`. #[cfg(test)] use crate::ipc::model::resource::{DeviceId, FileIdentity}` | //! | [`FileActivityCollector`] | `pub(super)` | `struct FileActivityCollector` | //! | [`create_file_activity_collector`] | `pub(super)` | `fn create_file_activity_collector() -> `[`Result`]`<`[`FileActivityCollector`]`>` | //! | [`poll_file_activity_collector`] | `pub(super)` | `fn poll_file_activity_collector(state: &`[`FileActivityCollector`]`) -> `[`Result`]`<()>` | //! | [`drain_file_activity_collector`] | `pub(super)` | `fn drain_file_activity_collector(state: &`[`FileActivityCollector`]`) -> `[`Vec`]`<`[`FileActivity`]`>` | //! | [`lost_file_activity_collector`] | `pub(super)` | `fn lost_file_activity_collector(state: &`[`FileActivityCollector`]`) -> u64` | //! //! [`FileActivityCollector`] (`pub(super)`): `struct FileActivityCollector`. #[cfg(test)] use crate::ipc::model::resource::{DeviceId, FileIdentity}` | //! | [`FileActivityCollector`] | `pub(super)` | `struct FileActivityCollector` | //! | [`create_file_activity_collector`] | `pub(super)` | `fn create_file_activity_collector() -> `[`Result`]`<`[`FileActivityCollector`]`>` | //! | [`poll_file_activity_collector`] | `pub(super)` | `fn poll_file_activity_collector(state: &`[`FileActivityCollector`]`) -> `[`Result`]`<()>` | //! | [`drain_file_activity_collector`] | `pub(super)` | `fn drain_file_activity_collector(state: &`[`FileActivityCollector`]`) -> `[`Vec`]`<`[`FileActivity`]`>` | //! | [`lost_file_activity_collector`] | `pub(super)` | `fn lost_file_activity_collector(state: &`[`FileActivityCollector`]`) -> u64` | //! //! [`FileActivityCollector`] (`pub(super)`): `struct FileActivityCollector`. #[cfg(test)] use crate::ipc::model::resource::{DeviceId, FileIdentity}` |
//! | [`FileActivityCollector`] | `pub(super)` | `struct FileActivityCollector` |
//! | [`create_file_activity_collector`] | `pub(super)` | `fn create_file_activity_collector() -> `[`Result`]`<`[`FileActivityCollector`]`>` |
//! | [`poll_file_activity_collector`] | `pub(super)` | `fn poll_file_activity_collector(state: &`[`FileActivityCollector`]`) -> `[`Result`]`<()>` |
//! | [`drain_file_activity_collector`] | `pub(super)` | `fn drain_file_activity_collector(state: &`[`FileActivityCollector`]`) -> `[`Vec`]`<`[`FileActivity`]`>` |
//! | [`lost_file_activity_collector`] | `pub(super)` | `fn lost_file_activity_collector(state: &`[`FileActivityCollector`]`) -> u64` |
//!

#[cfg(test)]
use crate::ipc::model::resource::DeviceId;
use crate::ipc::{model::resource::FileIdentity, schema::ProcessId};
use anyhow::{Context, Result};
use libbpf_rs::{MapCore, MapFlags, ObjectBuilder, RingBufferBuilder};
use std::sync::{Arc, Mutex};

pub(super) const COVERAGE: &str = "Regular-file read/write, pread/pwrite and vectored I/O, including page cache. Not physical disk traffic. mmap, io_uring, splice/sendfile and kernel workers are not observed.";
const MAX_AGGREGATES: usize = 8192;
use crate::ipc::activity::FileActivity;
#[derive(Default)]
struct Batch {
    events: Vec<FileActivity>,
    dropped: u64,
}
fn add_batch(state: &mut Batch, event: FileActivity) {
    if state.events.len() < MAX_AGGREGATES {
        state.events.push(event);
    } else {
        state.dropped += 1;
    }
}
/// Owns BPF resources and captures bounded regular-file I/O observations.
pub(super) struct FileActivityCollector {
    ring: libbpf_rs::RingBuffer<'static>,
    _links: Vec<libbpf_rs::Link>,
    obj: libbpf_rs::Object,
    batch: Arc<Mutex<Batch>>,
}
pub(super) fn create_file_activity_collector() -> Result<FileActivityCollector> {
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
        if let Some(event) = decode_file(data) {
            add_batch(&mut batch, event);
        } else {
            batch.dropped += 1;
        }
        0
    })?;
    Ok(FileActivityCollector {
        ring: builder.build()?,
        _links: links,
        obj,
        batch,
    })
}

pub(super) fn poll_file_activity_collector(state: &FileActivityCollector) -> Result<()> {
    let result = state.ring.consume_raw_n(8192);
    if result < 0 {
        return Err(std::io::Error::from_raw_os_error(-result).into());
    }
    Ok(())
}

pub(super) fn drain_file_activity_collector(state: &FileActivityCollector) -> Vec<FileActivity> {
    std::mem::take(&mut state.batch.lock().unwrap().events)
}

pub(super) fn lost_file_activity_collector(state: &FileActivityCollector) -> u64 {
    state
        .obj
        .maps()
        .find(|m| m.name() == "lost")
        .and_then(|m| m.lookup(&0u32.to_ne_bytes(), MapFlags::ANY).ok().flatten())
        .and_then(|bytes| bytes.try_into().ok())
        .map(u64::from_ne_bytes)
        .unwrap_or(0)
        + state.batch.lock().unwrap().dropped
}

fn decode_file(data: &[u8]) -> Option<FileActivity> {
    use crate::ipc::model::resource::device_id_from_kernel;

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
                * crate::ipc::model::process::ticks_per_second() as u128)
                / 1_000_000_000) as u64,
        },
        file: FileIdentity {
            device: device_id_from_kernel(dev),
            inode: u64_at(8),
            generation: u32_at(44),
        },
        path,
        write: u32_at(36) != 0,
        bytes,
        count: 1,
    })
}

#[cfg(test)]
mod tests {
    use {super::*, crate::http_server::analysis::activity::FileBatch};

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
        let mut activity = decode_file(&event()).unwrap();
        activity.path = None;
        assert_eq!(
            serde_json::to_value(&activity).unwrap(),
            serde_json::json!({
                "process_id": {"pid": 123, "start_time_ticks": crate::privileged_helper::process::ticks_per_second() as u64},
                "file": {"device":{"major":8,"minor":1},"inode":"42","generation":0}, "path": null, "write": false, "bytes": 7, "count": 1
            })
        );
    }

    #[test]
    fn decoding_and_path_failure() {
        let mut raw = event();
        let e = decode_file(&raw).unwrap();
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
            crate::privileged_helper::process::ticks_per_second() as u64
        );
        raw[40..44].copy_from_slice(&0u32.to_ne_bytes());
        assert!(decode_file(&raw).unwrap().path.is_none());
        raw[40..44].copy_from_slice(&4097u32.to_ne_bytes());
        assert!(decode_file(&raw).unwrap().path.is_none());
        assert!(decode_file(&raw[..100]).is_none());
        raw[24..32].fill(0);
        assert!(decode_file(&raw).is_none());
    }

    #[test]
    fn aggregation_separates_direction_and_process_lifetime_and_is_bounded() {
        use crate::http_server::analysis::activity::add_file_batch;

        let e = decode_file(&event()).unwrap();
        let mut batch = FileBatch::default();
        add_file_batch(&mut batch, e.clone());
        add_file_batch(&mut batch, e.clone());
        let mut write = e.clone();
        write.write = true;
        add_file_batch(&mut batch, write);
        let mut reused = e.clone();
        reused.process_id.start_time_ticks += 1;
        add_file_batch(&mut batch, reused);
        assert_eq!(batch.events.len(), 3);
        assert_eq!(batch.events[&(e.process_id, e.file, false)].bytes, 14);
        for i in 0..MAX_AGGREGATES {
            let mut next = e.clone();
            next.file.inode = i as u64 + 1000;
            add_file_batch(&mut batch, next);
        }
        assert_eq!(batch.events.len(), MAX_AGGREGATES);
        assert_eq!(batch.dropped, 3);
        add_file_batch(&mut batch, e.clone());
        assert_eq!(batch.events[&(e.process_id, e.file, false)].count, 3);
    }
}
