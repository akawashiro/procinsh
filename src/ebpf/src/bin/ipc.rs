//! Emits successful pipe and socket operations without counting socket peeks.
//!
//! # Interface
//!
//! - [`anon_pipe_read`] (`pub fn anon_pipe_read(ctx: FExitContext) -> u32`).
//! - [`anon_pipe_write`] (`pub fn anon_pipe_write(ctx: FExitContext) -> u32`).
//! - [`send`] (`pub fn send(ctx: BtfTracePointContext) -> u32`).
//! - [`recv`] (`pub fn recv(ctx: BtfTracePointContext) -> u32`).
//!
//! Context definitions: [`FExitContext`], [`BtfTracePointContext`].
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
    helpers::{bpf_get_current_pid_tgid, bpf_get_current_task_btf, bpf_ktime_get_ns},
    macros::{btf_tracepoint, fexit, map},
    maps::{Array, RingBuf},
    programs::{BtfTracePointContext, FExitContext},
};
use kernel::{layout::*, read};
use wire::IpcEvent;
#[map]
static events: RingBuf = RingBuf::with_byte_size(8 * 1024 * 1024, 0);
#[map]
static lost: Array<u64> = Array::with_max_entries(1, 0);
#[inline(always)]
unsafe fn emit(file: *const u8, ret: i64, kind: u32, write: u32) {
    if ret <= 0 || file.is_null() {
        return;
    }
    let inode: *const u8 = unsafe { read(file, FILE_F_INODE) };
    if inode.is_null() {
        return;
    }
    let Some(mut entry) = events.reserve::<IpcEvent>(0) else {
        if let Some(n) = lost.get_ptr_mut(0) {
            unsafe {
                core::intrinsics::atomic_xadd::<
                    u64,
                    u64,
                    { core::intrinsics::AtomicOrdering::Relaxed },
                >(n, 1);
            }
        }
        return;
    };
    let task = unsafe { bpf_get_current_task_btf() }.cast::<u8>();
    let leader: *const u8 = unsafe { read(task, TASK_STRUCT_GROUP_LEADER) };
    let sb: *const u8 = unsafe { read(inode, INODE_I_SB) };
    entry.write(IpcEvent {
        time: unsafe { bpf_ktime_get_ns() },
        start: unsafe { read(leader, TASK_STRUCT_START_BOOTTIME) },
        inode: unsafe { read(inode, INODE_I_INO) },
        device: unsafe { read::<u32>(sb, SUPER_BLOCK_S_DEV) } as u64,
        bytes: ret as u64,
        pid: (bpf_get_current_pid_tgid() >> 32) as u32,
        kind,
        write,
        worker: u32::from(
            unsafe { read::<u32>(task, TASK_STRUCT_FLAGS) } & (0x00200000 | 0x00000010) != 0,
        ),
    });
    entry.submit(0);
}
#[fexit(function = "anon_pipe_read")]
pub fn anon_pipe_read(ctx: FExitContext) -> u32 {
    unsafe {
        emit(read(ctx.arg(0), KIOCB_KI_FILP), ctx.arg(2), 1, 0);
    }
    0
}
#[fexit(function = "anon_pipe_write")]
pub fn anon_pipe_write(ctx: FExitContext) -> u32 {
    unsafe {
        emit(read(ctx.arg(0), KIOCB_KI_FILP), ctx.arg(2), 1, 1);
    }
    0
}
#[inline(always)]
unsafe fn socket_file(sk: *const u8) -> *const u8 {
    let socket: *const u8 = unsafe { read(sk, SOCK_SK_SOCKET) };
    unsafe { read(socket, SOCKET_FILE) }
}
#[btf_tracepoint(function = "sock_send_length")]
pub fn send(ctx: BtfTracePointContext) -> u32 {
    unsafe {
        emit(socket_file(ctx.arg(0)), ctx.arg::<i32>(1) as i64, 2, 1);
    }
    0
}
#[btf_tracepoint(function = "sock_recv_length")]
pub fn recv(ctx: BtfTracePointContext) -> u32 {
    if ctx.arg::<u32>(2) & 2 == 0 {
        unsafe {
            emit(socket_file(ctx.arg(0)), ctx.arg::<i32>(1) as i64, 2, 0);
        }
    }
    0
}
#[unsafe(link_section = "license")]
#[unsafe(no_mangle)]
static LICENSE: [u8; 4] = *b"GPL\0";
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
