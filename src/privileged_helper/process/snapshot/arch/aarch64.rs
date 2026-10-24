//! AArch64 placeholders with the same interface as `x86_64.rs`. Register capture,
//! perf sampling and naming are not implemented yet, so snapshots report an error.
//!
//! To implement: read `struct user_pt_regs` (X0–X30, SP, PC, PSTATE) with
//! PTRACE_GETREGSET / NT_PRSTATUS, sample the same registers with
//! PERF_REG_ARM64_* (whose order differs from ptrace), and name them with
//! PC → pc, SP → sp, X29 → fp, X30 → lr and PSTATE → flags.
use crate::privileged_helper::process::snapshot::registers::RegisterRole;
use anyhow::{Result, bail};

/// Zero makes perf sampling refuse to open; see `PerfEventReader::open_attr`.
pub(in crate::privileged_helper::process::snapshot) const PERF_REGS_MASK: u64 = 0;
/// Indices of PC, SP and FP in the register set.
pub(in crate::privileged_helper::process::snapshot) const PC: usize = 0;
pub(in crate::privileged_helper::process::snapshot) const SP: usize = 0;
pub(in crate::privileged_helper::process::snapshot) const FP: usize = 0;

/// Displayed registers in order: name, role and register-set index.
pub(in crate::privileged_helper::process::snapshot) const REGISTERS: &[(
    &str,
    RegisterRole,
    usize,
)] = &[];

pub(in crate::privileged_helper::process::snapshot) fn read_registers(
    _tid: i32,
) -> Result<[u64; 24]> {
    bail!("AArch64 register capture is not implemented yet")
}
