//! Non-stopping perf samples, sampled-stack unwind and best-effort live disassembly.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind |
//! | --- | --- | --- |
//! | [`Sampler`] | `pub(super)` | `struct Sampler` |
//! | [`ThreadSample`] | `pub(super)` | Serializable latest sample, age, CPU, loss and errors |
//!
//! - [`Sampler::new`]: `fn new(id: ProcessId) -> Self`.
//! - [`Sampler::poll`]: `fn poll(&mut self, maps: &[MemoryMap]) -> anyhow::Result<()>`.
//! - [`Sampler::latest`]: `fn latest(&self) -> Vec<ThreadSample>`.
//!
//! [`ThreadSample`] fields (private, serialized): `tid: i32, sampled_at: Option<u64>,
//! sample_source: Option<perf::SampleSource>, sample_age_ms: Option<u64>, cpu: Option<u32>, lost_samples: u64, registers: Vec<Register>,
//! call_stack: Vec<StackFrame>, disassembly: Option<Disassembly>, unwind_stop: String, error: Option<String>`.
//!
//! Internal perf interface (`pub(super)`):
//! - [`perf::REGS_MASK`]: `const REGS_MASK: u64`, GPR/RIP/RSP/RBP/RFLAGS mask.
//! - [`perf::SampleSource`]: `enum SampleSource { CpuClock, ContextSwitch { preempted: bool } }`.
//! - [`perf::Event::context_switch`]: `fn context_switch(tid: i32) -> anyhow::Result<Self>`.
//! - [`perf::Sample`]: `struct Sample { source: SampleSource, tid: i32, time_ns: u64, cpu: u32, registers: RegisterSet, stack: Vec<u8> }`.
//! - [`perf::Event`]: `struct Event`, fd/mmap owner, with `lost: u64`.
//! - [`perf::Event::open`]: `fn open(tid: i32) -> anyhow::Result<Self>`.
//! - [`perf::Event::drain`]: `fn drain(&mut self) -> anyhow::Result<Option<Sample>>`.
//!
//! Register payload fields (`pub(super)` in [`registers`]):
//! - [`registers::Register`]: `kind: MemoryKind`, `mapping: Option<RegisterMapping>`, `offset: Option<u64>` (hex string or null in JSON).
//! - [`registers::RegisterMapping`]: `pathname: Option<String>, readable: bool, writable: bool, executable: bool, private: bool`.
//! - [`registers::classify`]: `fn classify(name: &str, value: u64, maps: &[MemoryMap]) -> Register`.
//! - [`registers::from_sample`]: `fn from_sample(r: &RegisterSet, maps: &[MemoryMap]) -> Vec<Register>`.
//!
//! - [`registers::RegisterSet`]: `struct RegisterSet(pub(super) [u64; 24])`, indexed by PERF_REG_X86_* .
//! - [`disasm::Disassembly::capture`]: `fn capture(pid: i32, rip: u64, maps: &[MemoryMap]) -> Self`.
//! - [`unwind_fp::walk`]: `fn walk(rip: u64, rsp: u64, rbp: u64, maps: &[MemoryMap], read: impl FnMut(u64) -> Option<[u8; 16]>) -> (Vec<StackFrame>, String)`.
//!
//! Memory kinds are defined in [`super::maps::MemoryKind`].
mod capture;
mod disasm;
mod perf;
mod registers;
mod stack;
mod symbol;
mod unwind_fp;
pub(super) use capture::{Sampler, ThreadSample};
