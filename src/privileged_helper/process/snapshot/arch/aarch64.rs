//! Native Linux AArch64 register capture using NT_PRSTATUS and optional NT_ARM_PAC_MASK.
//!
//! # Interface
//! All entries have `pub(in crate::privileged_helper::process::snapshot)` visibility.
//! - [`PERF_REGS_MASK`]: `const PERF_REGS_MASK: u64`.
//! - [`read_registers`]: `fn read_registers(tid: i32) -> anyhow::Result<RegisterSet>`.
//!
//! Captured values use [`RegisterSet`].
use crate::ipc::registers::RegisterSet;
use crate::privileged_helper::process::snapshot::ptrace::request;
use anyhow::{Result, ensure};
use std::ptr;

// PERF_REG_ARM64_X0..X29, LR, SP and PC. PSTATE is not part of perf's ABI.
pub(in crate::privileged_helper::process::snapshot) const PERF_REGS_MASK: u64 = (1u64 << 33) - 1;

/// Reads registers from a stopped TID; an unavailable PAC regset does not fail capture.
pub(in crate::privileged_helper::process::snapshot) fn read_registers(
    tid: i32,
) -> Result<RegisterSet> {
    // Linux user_pt_regs consists of X0-X30, SP, PC and PSTATE, in that order.
    let mut values = [0u64; 34];
    let mut iov = libc::iovec {
        iov_base: values.as_mut_ptr().cast(),
        iov_len: std::mem::size_of_val(&values),
    };
    request(
        libc::PTRACE_GETREGSET,
        tid,
        libc::NT_PRSTATUS as usize,
        ptr::from_mut(&mut iov) as usize,
    )?;
    ensure!(
        iov.iov_len == std::mem::size_of_val(&values),
        "Incomplete AArch64 register set"
    );
    // Linux user_pac_mask contains data_mask, then insn_mask.
    let mut masks = [0u64; 2];
    let mut pac_iov = libc::iovec {
        iov_base: masks.as_mut_ptr().cast(),
        iov_len: std::mem::size_of_val(&masks),
    };
    let code_pac_mask = request(
        libc::PTRACE_GETREGSET,
        tid,
        0x406,
        ptr::from_mut(&mut pac_iov) as usize,
    )
    .ok()
    .filter(|_| pac_iov.iov_len == std::mem::size_of_val(&masks))
    .map(|_| masks[1]);
    Ok(RegisterSet {
        values: values.to_vec(),
        flags_available: true,
        code_pac_mask,
    })
}
