//! Fixed-layout map values and ring-buffer records shared with userspace.
//!
//! # Interface
//!
//! All types are `pub(crate) struct`: [`ProcessKey`], [`CpuSlot`], [`CpuTotal`],
//! [`IpcEvent`], [`FileEvent`] and [`PendingIo`]. Fields and ABI assertions are
//! defined below; only integer fields and explicitly padded byte arrays cross BPF.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub(crate) struct ProcessKey {
    pub start: u64,
    pub pid: u32,
    pub pad: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct CpuSlot {
    pub process: ProcessKey,
    pub since: u64,
    pub tid: u32,
    pub pad: u32,
}
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct CpuTotal {
    pub runtime: u64,
    pub switches: u64,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct IpcEvent {
    pub time: u64,
    pub start: u64,
    pub inode: u64,
    pub device: u64,
    pub bytes: u64,
    pub pid: u32,
    pub kind: u32,
    pub write: u32,
    pub worker: u32,
}
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct FileEvent {
    pub start: u64,
    pub inode: u64,
    pub device: u64,
    pub bytes: u64,
    pub pid: u32,
    pub write: u32,
    pub path_len: u32,
    pub generation: u32,
    pub path: [u8; 4096],
}
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct PendingIo {
    pub event: FileEvent,
    pub file: u64,
    pub depth: u32,
    pub pad: u32,
}
const _: () = {
    assert!(core::mem::size_of::<ProcessKey>() == 16);
    assert!(core::mem::size_of::<CpuSlot>() == 32);
    assert!(core::mem::size_of::<CpuTotal>() == 16);
    assert!(core::mem::size_of::<IpcEvent>() == 56);
    assert!(core::mem::size_of::<FileEvent>() == 4144);
    assert!(core::mem::size_of::<PendingIo>() == 4160);
};
