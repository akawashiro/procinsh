//! Stop, capture, resume, then decode and symbolize a process snapshot.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind |
//! | --- | --- | --- |
//! | [`Snapshotter`] | `pub(super)` | `struct `[`Snapshotter`] |
mod capture;
mod disasm;
mod ptrace;
mod registers;
mod stack;
mod symbol;
mod unwind_fp;
pub(super) use capture::Snapshotter;
