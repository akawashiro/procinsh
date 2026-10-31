# Native Linux AArch64 inspection

Thread snapshots support native little-endian 64-bit AArch64 and x86-64 processes.
32-bit compatibility processes are unsupported. The HTTP process classifies the
helper's acquired executable header before interpreting register or stack data.

## Registers and stack acquisition

The privileged helper uses `PTRACE_GETREGSET(NT_PRSTATUS)` for initial and periodic
stopped-thread snapshots. AArch64 values are X0–X30, SP, PC and PSTATE. Perf samples
use `PERF_REG_ARM64_*` indices 0–32: X0–X29, LR, SP and PC. Perf does not supply
PSTATE, so the register table omits it for those samples. Values from separate
captures are never combined into a register snapshot.

The private IPC register value includes its native register count, flags
availability and optional code pointer-authentication mask. Both IPC and internal
JSON decoding validate the register count. x86-64 retains its 24-slot perf layout
and 18 displayed registers; AArch64 displays 34 ptrace or 33 perf registers.

When available, `NT_ARM_PAC_MASK` supplies the bits occupied by instruction-pointer
authentication. The helper retains that metadata per thread for perf analysis;
TID reuse creates a new thread state. Unavailable masks do not fail register
capture. The unwinder strips only bits identified by the kernel; it does not
infer an address width or guess a mask.

## Analysis

Framehop selects the native x86-64 or AArch64 implementation and unwinds captured
stack bytes with cached ELF CFI, falling back to frame pointers. AArch64 starts
with the sampled PC, SP, X29 and X30. Analysis never reads additional live stack
memory. Existing snapshot coverage checks and the 256-frame limit apply.

Capstone decodes both instruction sets in the unprivileged HTTP process. The
build enables only x86 and ARM64 engines and statically links the bundled C
library. x86-64 uses Intel syntax. AArch64 uses aligned four-byte A64 instructions.
Decoding starts at the sampled PC, uses its runtime address for relative
references, and stops at 256 captured bytes or 32 instructions. Invalid or
unsupported bytes stop decoding without skipping data. Capstone cannot reliably
distinguish invalid x86 encodings from truncated instructions, so that diagnostic
names both possibilities. Capture errors are retained alongside decode errors.
Instruction bytes are read after sampling and may differ for mutable code.

## Validation

Both architectures run `ordinary`, `checks`, `kernel` and `browser` through
`scripts/docker_test.sh`. Rust tests require the configured Docker capabilities
and run without permission-based skips. Decoder tests exercise both instruction
sets on either host; native live tests cover stopped and perf snapshots, blocked
threads, frame-pointer omission, shared libraries and source resolution.
