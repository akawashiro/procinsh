//! x86-64 register capture and naming. Register sets are indexed by Linux
//! PERF_REG_X86_* so ptrace and perf samples share one layout.
use crate::privileged_helper::process::snapshot::{ptrace::request, registers::RegisterRole};
use anyhow::{Result, ensure};
use std::ptr;

// AX/BX/CX/DX/SI/DI/BP/SP/IP/FLAGS and R8–R15, in kernel register order.
pub(in crate::privileged_helper::process::snapshot) const PERF_REGS_MASK: u64 =
    0x3ff | (0xff << 16);
/// PERF_REG_X86_IP, PERF_REG_X86_SP and PERF_REG_X86_BP.
pub(in crate::privileged_helper::process::snapshot) const PC: usize = 8;
pub(in crate::privileged_helper::process::snapshot) const SP: usize = 7;
pub(in crate::privileged_helper::process::snapshot) const FP: usize = 6;

/// Displayed registers in order: name, role and PERF_REG_X86_* index.
pub(in crate::privileged_helper::process::snapshot) const REGISTERS: &[(
    &str,
    RegisterRole,
    usize,
)] = {
    use RegisterRole::{Flags, Fp, General, Pc, Sp};
    &[
        ("RIP", Pc, 8),
        ("RSP", Sp, 7),
        ("RBP", Fp, 6),
        ("RAX", General, 0),
        ("RBX", General, 1),
        ("RCX", General, 2),
        ("RDX", General, 3),
        ("RSI", General, 4),
        ("RDI", General, 5),
        ("R8", General, 16),
        ("R9", General, 17),
        ("R10", General, 18),
        ("R11", General, 19),
        ("R12", General, 20),
        ("R13", General, 21),
        ("R14", General, 22),
        ("R15", General, 23),
        ("RFLAGS", Flags, 9),
    ]
};

/// Reads a ptrace-stopped thread's registers in PERF_REG_X86_* order.
pub(in crate::privileged_helper::process::snapshot) fn read_registers(
    tid: i32,
) -> Result<[u64; 24]> {
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
    Ok(values)
}
