//! Shared captured registers values.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`RegisterMapping`] | `pub(super)` | `struct RegisterMapping` |
//! | [`Register`] | `pub(super)` | `struct Register` |
//! | [`RegisterSet`] | `pub(super)` | `struct RegisterSet` |

pub(super) use crate::ipc::registers::*;

#[cfg(test)]
pub(super) use crate::http_server::analysis::snapshot::registers::classify_sample_registers;
