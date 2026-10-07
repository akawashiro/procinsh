//! Collects regular-file operations and captures paths while the file is live.
//!
//! # Interface
//!
//! | Definition | Visibility and signature |
//! | --- | --- |
//! | [`file_path`] | `pub fn file_path(ctx: FEntryContext) -> u32` |
//! | [`file_exit`] | `pub fn file_exit(ctx: BtfTracePointContext) -> u32` |
//! | [`enter_vfs_read`] | `pub fn enter_vfs_read(ctx: FEntryContext) -> u32` |
//! | [`exit_vfs_read`] | `pub fn exit_vfs_read(ctx: FExitContext) -> u32` |
//! | [`enter_vfs_write`] | `pub fn enter_vfs_write(ctx: FEntryContext) -> u32` |
//! | [`exit_vfs_write`] | `pub fn exit_vfs_write(ctx: FExitContext) -> u32` |
//! | [`enter_vfs_readv`] | `pub fn enter_vfs_readv(ctx: FEntryContext) -> u32` |
//! | [`exit_vfs_readv`] | `pub fn exit_vfs_readv(ctx: FExitContext) -> u32` |
//! | [`enter_vfs_writev`] | `pub fn enter_vfs_writev(ctx: FEntryContext) -> u32` |
//! | [`exit_vfs_writev`] | `pub fn exit_vfs_writev(ctx: FExitContext) -> u32` |
//!
//! Context definitions: [`FEntryContext`], [`FExitContext`], [`BtfTracePointContext`].
#![no_std]
#![no_main]
// Keep existing kernel map names; each binary uses only part of the shared ABI.
#![allow(non_upper_case_globals)]
#![feature(core_intrinsics)]
#![allow(internal_features)]
#[path = "../kernel.rs"]
mod kernel;
#[allow(dead_code)]
#[path = "../wire.rs"]
mod wire;
use aya_ebpf::{
    helpers::{bpf_d_path, bpf_get_current_pid_tgid, bpf_get_current_task_btf},
    macros::{btf_tracepoint, fentry, fexit, map},
    maps::{Array, HashMap, RingBuf},
    programs::{BtfTracePointContext, FEntryContext, FExitContext},
};
use kernel::{layout::*, read};
use wire::{FileEvent, PendingIo};
#[map]
static pending: HashMap<u64, PendingIo> = HashMap::with_max_entries(4096, 0);
#[map]
static events: RingBuf = RingBuf::with_byte_size(8 * 1024 * 1024, 0);
#[map]
static lost: Array<u64> = Array::with_max_entries(1, 0);
// Keep the 4160-byte empty value in read-only storage, never on the BPF stack.
static EMPTY: PendingIo = PendingIo {
    event: FileEvent {
        start: 0,
        inode: 0,
        device: 0,
        bytes: 0,
        pid: 0,
        write: 0,
        path_len: 0,
        generation: 0,
        path: [0; 4096],
    },
    file: 0,
    depth: 0,
    pad: 0,
};
#[inline(always)]
fn dropped() {
    if let Some(n) = lost.get_ptr_mut(0) {
        unsafe {
            core::intrinsics::atomic_xadd::<u64, u64, { core::intrinsics::AtomicOrdering::Relaxed }>(
                n, 1,
            );
        }
    }
}
#[expect(
    clippy::needless_borrows_for_generic_args,
    reason = "borrow the large static EMPTY value to avoid a BPF stack copy"
)]
#[inline(always)]
unsafe fn begin(file: *const u8, write: u32) {
    let tid = bpf_get_current_pid_tgid();
    if let Some(p) = pending.get_ptr_mut(tid) {
        unsafe {
            (*p).depth += 1;
        }
        return;
    }
    let task = unsafe { bpf_get_current_task_btf() }.cast::<u8>();
    let flags: u32 = unsafe { read(task, TASK_STRUCT_FLAGS) };
    if file.is_null() || flags & (0x00200000 | 0x00000010) != 0 {
        return;
    }
    let inode: *const u8 = unsafe { read(file, FILE_F_INODE) };
    let mode: u16 = unsafe { read(inode, INODE_I_MODE) };
    if inode.is_null() || mode & 0o170000 != 0o100000 {
        return;
    }
    if pending.insert(tid, &EMPTY, 1).is_err() {
        dropped();
        return;
    }
    let Some(p) = pending.get_ptr_mut(tid) else {
        return;
    };
    let leader: *const u8 = unsafe { read(task, TASK_STRUCT_GROUP_LEADER) };
    let sb: *const u8 = unsafe { read(inode, INODE_I_SB) };
    unsafe {
        (*p).file = file as u64;
        (*p).depth = 1;
        (*p).event.start = read(leader, TASK_STRUCT_START_BOOTTIME);
        (*p).event.pid = (tid >> 32) as u32;
        (*p).event.inode = read(inode, INODE_I_INO);
        (*p).event.device = read::<u32>(sb, SUPER_BLOCK_S_DEV) as u64;
        (*p).event.generation = read(inode, INODE_I_GENERATION);
        (*p).event.write = write;
    }
}
#[fentry(function = "security_file_permission")]
pub fn file_path(ctx: FEntryContext) -> u32 {
    let file: *const u8 = ctx.arg(0);
    let tid = bpf_get_current_pid_tgid();
    let Some(p) = pending.get_ptr_mut(tid) else {
        return 0;
    };
    unsafe {
        if (*p).file != file as u64 || (*p).depth != 1 {
            return 0;
        }
        // Preserve the BTF file pointer from ctx: the helper needs an embedded
        // kernel path, not a copied path on our stack.
        let len = bpf_d_path(
            file.add(FILE_F_PATH).cast_mut().cast(),
            core::ptr::addr_of_mut!((*p).event.path).cast(),
            4096,
        );
        (*p).event.path_len = if len > 0 && len <= 4096 {
            len as u32
        } else {
            0
        };
    }
    0
}
#[inline(always)]
unsafe fn finish(ret: i64) {
    let tid = bpf_get_current_pid_tgid();
    let Some(p) = pending.get_ptr_mut(tid) else {
        return;
    };
    unsafe {
        if (*p).depth > 1 {
            (*p).depth -= 1;
            return;
        }
        if ret > 0 {
            (*p).event.bytes = ret as u64;
            if events.output::<FileEvent>(&(*p).event, 0).is_err() {
                dropped();
            }
        }
    }
    let _ = pending.remove(tid);
}
macro_rules! operation {
    ($enter:ident, $exit:ident, $target:literal, $write:expr, $ret_index:expr) => {
        #[fentry(function = $target)]
        pub fn $enter(ctx: FEntryContext) -> u32 {
            unsafe {
                begin(ctx.arg(0), $write);
            }
            0
        }
        #[fexit(function = $target)]
        pub fn $exit(ctx: FExitContext) -> u32 {
            unsafe {
                finish(ctx.arg($ret_index));
            }
            0
        }
    };
}
operation!(enter_vfs_read, exit_vfs_read, "vfs_read", 0, 4);
operation!(enter_vfs_write, exit_vfs_write, "vfs_write", 1, 4);
operation!(enter_vfs_readv, exit_vfs_readv, "vfs_readv", 0, 5);
operation!(enter_vfs_writev, exit_vfs_writev, "vfs_writev", 1, 5);
#[btf_tracepoint(function = "sched_process_exit")]
pub fn file_exit(_ctx: BtfTracePointContext) -> u32 {
    let _ = pending.remove(bpf_get_current_pid_tgid());
    0
}
#[unsafe(link_section = "license")]
#[unsafe(no_mangle)]
static LICENSE: [u8; 4] = *b"GPL\0";
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
