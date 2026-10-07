//! Aggregates scheduled runtime in per-CPU maps and removes exited processes.
//!
//! # Interface
//!
//! - [`schedule`] (`pub fn schedule(ctx: BtfTracePointContext) -> u32`): sched_switch.
//! - [`process_exit`] (`pub fn process_exit(ctx: BtfTracePointContext) -> u32`): sched_process_exit.
//!
//! Context definition: [`BtfTracePointContext`].
#![no_std]
#![no_main]
// Keep existing kernel map names; each binary uses only part of the shared ABI.
#![allow(non_upper_case_globals)]
#[path = "../kernel.rs"]
mod kernel;
#[allow(dead_code)]
#[path = "../wire.rs"]
mod wire;
use aya_ebpf::{
    helpers::bpf_ktime_get_ns,
    macros::{btf_tracepoint, map},
    maps::{LruPerCpuHashMap, PerCpuArray},
    programs::BtfTracePointContext,
};
use kernel::{layout::*, process_key, read};
use wire::{CpuSlot, CpuTotal, ProcessKey};
#[map]
static cpu_current: PerCpuArray<CpuSlot> = PerCpuArray::with_max_entries(1, 0);
#[map]
static cpu_totals: LruPerCpuHashMap<ProcessKey, CpuTotal> =
    LruPerCpuHashMap::with_max_entries(65536, 0);
#[btf_tracepoint(function = "sched_switch")]
pub fn schedule(ctx: BtfTracePointContext) -> u32 {
    let prev: *const u8 = ctx.arg(1);
    let next: *const u8 = ctx.arg(2);
    let now = unsafe { bpf_ktime_get_ns() };
    let Some(slot) = cpu_current.get_ptr_mut(0) else {
        return 0;
    };
    unsafe {
        let previous = process_key(prev);
        if previous.pid != 0 && (*slot).process == previous && now >= (*slot).since {
            if cpu_totals.get_ptr_mut(previous).is_none() {
                let _ = cpu_totals.insert(
                    previous,
                    CpuTotal {
                        runtime: 0,
                        switches: 0,
                    },
                    1,
                );
            }
            if let Some(total) = cpu_totals.get_ptr_mut(previous) {
                (*total).runtime += now - (*slot).since;
                (*total).switches += 1;
            }
        }
        (*slot).process = process_key(next);
        (*slot).since = now;
        (*slot).tid = read(next, TASK_STRUCT_PID);
    }
    0
}
#[btf_tracepoint(function = "sched_process_exit")]
pub fn process_exit(ctx: BtfTracePointContext) -> u32 {
    if ctx.arg::<u32>(1) != 0 {
        let key = unsafe { process_key(ctx.arg(0)) };
        let _ = cpu_totals.remove(key);
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
