//! Initial and ten-second ptrace snapshots, non-stopping perf samples,
//! raw register/stack snapshots and best-effort instruction-byte capture.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind |
//! | --- | --- | --- |
//! | [`Sampler`] | `pub(super)` | `struct `[`Sampler`] |

mod arch;
mod capture;
mod instruction_bytes;
mod perf;
mod ptrace;
mod sample;
#[cfg(test)]
mod unwind_fp;
pub(super) use capture::Sampler;
