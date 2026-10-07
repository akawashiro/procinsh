//! Reads fields using constants generated from this kernel's BTF.
//!
//! # Interface
//!
//! - [`read`] (`pub(crate) unsafe fn read<T: Copy>(base: *const u8, offset: usize) -> T`)
//!   probes a kernel field; failures produce a zero value, like BPF_CORE_READ.
//! - [`process_key`] (`pub(crate) unsafe fn process_key(task: *const u8) -> ProcessKey`)
//!   reads a process leader's identity ([`ProcessKey`]).
//! - `layout` (`pub(crate) mod`): generated offset constants, only for this kernel.
use crate::wire::ProcessKey;
use aya_ebpf::helpers::bpf_probe_read_kernel;
#[allow(dead_code)]
pub(crate) mod layout {
    include!(env!("PROCINSH_KERNEL_LAYOUT"));
}

/// `base` must be a kernel pointer, and `T` must be a fixed-width integer or
/// pointer whose width was validated by KernelLayoutReader. No Rust references
/// into kernel memory are created. Null pointers retain the C sensor fallback.
#[inline(always)]
pub(crate) unsafe fn read<T: Copy>(base: *const u8, offset: usize) -> T {
    if base.is_null() {
        return unsafe { core::mem::zeroed() };
    }
    unsafe { bpf_probe_read_kernel(base.add(offset).cast::<T>()).unwrap_or(core::mem::zeroed()) }
}
#[allow(dead_code)]
#[inline(always)]
pub(crate) unsafe fn process_key(task: *const u8) -> ProcessKey {
    let leader: *const u8 = unsafe { read(task, layout::TASK_STRUCT_GROUP_LEADER) };
    ProcessKey {
        start: unsafe { read(leader, layout::TASK_STRUCT_START_BOOTTIME) },
        pid: unsafe { read(leader, layout::TASK_STRUCT_TGID) },
        pad: 0,
    }
}
