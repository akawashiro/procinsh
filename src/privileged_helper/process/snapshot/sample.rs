//! Native register and stack samples with a monotonic collection clock.
//!
//! # Interface
//! - [`RawSample`] (`pub(super)`): `struct RawSample`.
//! - [`monotonic_ns`] (`pub(super)`): `fn monotonic_ns() -> u64`.

use crate::ipc::acquisition::SampleSource;
use crate::ipc::registers::RegisterSet;
pub(super) struct RawSample {
    pub(super) source: SampleSource,
    pub(super) tid: i32,
    pub(super) time_ns: u64,
    pub(super) cpu: Option<u32>,
    pub(super) registers: RegisterSet,
    pub(super) stack: Vec<u8>,
}

pub(super) fn monotonic_ns() -> u64 {
    let mut time: libc::timespec = unsafe { std::mem::zeroed() };
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time);
    }
    time.tv_sec as u64 * 1_000_000_000 + time.tv_nsec as u64
}
