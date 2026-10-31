//! x86-64 register capture. Register sets are indexed by Linux
//! PERF_REG_X86_* so ptrace and perf samples share one layout.
//!
//! # Interface
//! All entries have `pub(in crate::privileged_helper::process::snapshot)` visibility.
//! - [`PERF_REGS_MASK`]: `const PERF_REGS_MASK: u64`.
//! - [`read_registers`]: `fn read_registers(tid: i32) -> anyhow::Result<RegisterSet>`.
use crate::ipc::registers::RegisterSet;
use crate::privileged_helper::process::snapshot::ptrace::request;
use anyhow::{Result, ensure};
use std::ptr;

// AX/BX/CX/DX/SI/DI/BP/SP/IP/FLAGS and R8–R15, in kernel register order.
pub(in crate::privileged_helper::process::snapshot) const PERF_REGS_MASK: u64 =
    0x3ff | (0xff << 16);
/// Reads a ptrace-stopped thread's registers in PERF_REG_X86_* order.
pub(in crate::privileged_helper::process::snapshot) fn read_registers(
    tid: i32,
) -> Result<RegisterSet> {
    let mut regs: libc::user_regs_struct = unsafe { std::mem::zeroed() };
    let mut iov = libc::iovec {
        iov_base: ptr::from_mut(&mut regs).cast(),
        iov_len: std::mem::size_of_val(&regs),
    };
    request(
        libc::PTRACE_GETREGSET,
        tid,
        libc::NT_PRSTATUS as usize,
        ptr::from_mut(&mut iov) as usize,
    )?;
    ensure!(
        iov.iov_len == std::mem::size_of_val(&regs),
        "Incomplete register set"
    );
    let mut values = [0; 24];
    values[..10].copy_from_slice(&[
        regs.rax,
        regs.rbx,
        regs.rcx,
        regs.rdx,
        regs.rsi,
        regs.rdi,
        regs.rbp,
        regs.rsp,
        regs.rip,
        regs.eflags,
    ]);
    values[16..].copy_from_slice(&[
        regs.r8, regs.r9, regs.r10, regs.r11, regs.r12, regs.r13, regs.r14, regs.r15,
    ]);
    Ok(RegisterSet {
        values: values.to_vec(),
        flags_available: true,
        code_pac_mask: None,
    })
}
