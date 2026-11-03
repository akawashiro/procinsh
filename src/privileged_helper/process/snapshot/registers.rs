//! Shared captured registers values.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`RegisterMapping`] | `pub(super)` | `struct RegisterMapping` |
//! | [`Register`] | `pub(super)` | `struct Register` |
//! | [`RegisterSet`] | `pub(super)` | `struct RegisterSet` |

#[allow(unused_imports, reason = "Preserve the complete façade interface")]
pub(super) use crate::ipc::registers::{Register, RegisterMapping, RegisterRole, RegisterSet};

#[cfg(test)]
pub(super) use crate::http_server::analysis::snapshot::registers::from_sample;
