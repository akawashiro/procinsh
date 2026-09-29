//! Stop, capture, resume, then decode and symbolize a process snapshot.
//!
//! # Interface
//!
//! Re-export visibility: `pub(super)`. Follow the definition link, then **Source**, for the implementation.
//!
//! | Definition | Kind |
//! | --- | --- |
//! | [`Snapshotter`] | `struct Snapshotter` |
mod capture;
mod disasm;
mod ptrace;
mod registers;
mod stack;
mod symbol;
mod unwind_fp;
pub(super) use capture::Snapshotter;
