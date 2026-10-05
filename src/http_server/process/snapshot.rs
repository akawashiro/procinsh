//! Initial and ten-second ptrace snapshots, non-stopping perf samples,
//! sampled-stack unwind and best-effort live disassembly.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind |
//! | --- | --- | --- |
//! | [`Sampler`] | `pub(super)` | `struct `[`Sampler`] |
//! | [`ThreadSample`] | `pub(super)` | `struct `[`ThreadSample`] |

mod capture;
mod disasm;
mod perf;
mod ptrace;
mod registers;
mod sample;
mod stack;
mod symbol;
mod unwind;
#[cfg(test)]
mod unwind_fp;
pub(super) use capture::{Sampler, ThreadSample};
