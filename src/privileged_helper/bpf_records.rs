//! Decode fixed-layout native BPF records after bounded kernel reads.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`decode_file`] | `pub` | `fn decode_file(data: &[u8]) -> `[`Option`]`<`[`FileActivity`]`>` |
//! | [`decode_ipc`] | `pub` | `fn decode_ipc(bytes: &[u8]) -> `[`Option`]`<`[`IpcEvent`]`>` |
//! | [`decode_signal`] | `pub` | `fn decode_signal(bytes: &[u8]) -> `[`Option`]`<`[`SignalSample`]`>` |

use crate::ipc::activity::{FileActivity, IpcEvent, SignalSample};
use crate::ipc::model::resource::FileIdentity;
use crate::ipc::schema::ProcessId;

pub fn decode_file(data: &[u8]) -> Option<FileActivity> {
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

pub fn decode_ipc(bytes: &[u8]) -> Option<IpcEvent> {
    if bytes.len() != 56 {
        return None;
    }
    let u64at = |i| u64::from_ne_bytes(bytes[i..i + 8].try_into().unwrap());
    let u32at = |i| u32::from_ne_bytes(bytes[i..i + 4].try_into().unwrap());
    Some(IpcEvent {
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

pub fn decode_signal(bytes: &[u8]) -> Option<SignalSample> {
    if bytes.len() != 40 {
        return None;
    }
    let u64at = |i| u64::from_ne_bytes(bytes[i..i + 8].try_into().unwrap());
    let u32at = |i| u32::from_ne_bytes(bytes[i..i + 4].try_into().unwrap());
    Some(SignalSample {
        timestamp_ns: u64at(0),
        src_start: u64at(8),
        dst_start: u64at(16),
        src_pid: u32at(24),
        dst_pid: u32at(28),
        signal: u32at(32) as i32,
    })
}
