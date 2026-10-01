//! Stop, capture, resume, then decode and symbolize a process snapshot.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind |
//! | --- | --- | --- |
//! | [`Snapshotter`] | `pub(super)` | `struct `[`Snapshotter`] |
//! Register payload fields (`pub(super)` in [`registers`]):
//! - [`registers::Register`]: `kind: MemoryKind`, `mapping: Option<RegisterMapping>`, `offset: Option<u64>` (hex string or null in JSON).
//! - [`registers::RegisterMapping`]: `pathname: Option<String>, readable: bool, writable: bool, executable: bool, private: bool`.
//! - [`registers::classify`]: `fn classify(name: &str, value: u64, maps: &[MemoryMap]) -> Register`.
//! - [`registers::from_raw`]: `fn from_raw(r: &libc::user_regs_struct, maps: &[MemoryMap]) -> Vec<Register>`.
//! Memory kinds are defined in [`super::maps::MemoryKind`].
mod capture;
mod disasm;
mod ptrace;
mod registers;
mod stack;
mod symbol;
mod unwind_fp;
pub(super) use capture::Snapshotter;
