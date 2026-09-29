//! Stop, capture, resume, then decode and symbolize a process snapshot.
mod capture;
mod disasm;
mod ptrace;
mod registers;
mod stack;
mod symbol;
mod unwind_fp;
pub use capture::Snapshotter;
