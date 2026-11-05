//! Native register acquisition layout; executable headers are passed to main as bytes.

//!
//! # Interface
//! | Definition | Visibility | Kind |
//! | --- | --- | --- |
//! | `x86_64` (x86-64 builds; definition: [`native`]) | `pub(super)` | `mod x86_64` |
//! | `aarch64` (AArch64 builds; definition: [`native`]) | `pub(super)` | `mod aarch64` |
//! | [`native`] | `pub(super)` | Native module alias |
#[cfg(target_arch = "aarch64")]
pub(super) mod aarch64;
#[cfg(target_arch = "x86_64")]
pub(super) mod x86_64;
#[cfg(target_arch = "aarch64")]
pub(super) use aarch64 as native;
#[cfg(target_arch = "x86_64")]
pub(super) use x86_64 as native;
