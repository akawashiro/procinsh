//! Native register acquisition layout; executable headers are passed to main as bytes.
//!
//! # Interface
//! | Definition | Visibility | Kind |
//! | --- | --- | --- |
#![cfg_attr(
    target_arch = "x86_64",
    doc = "| [`x86_64`] (x86-64 builds) | `pub(super)` | `mod x86_64` |"
)]
#![cfg_attr(
    target_arch = "aarch64",
    doc = "| [`aarch64`] (AArch64 builds) | `pub(super)` | `mod aarch64` |"
)]
#[cfg(target_arch = "aarch64")]
pub(super) mod aarch64;
#[cfg(target_arch = "x86_64")]
pub(super) mod x86_64;
