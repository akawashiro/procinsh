//! Shared captured stack values.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`StackFrame`] | `pub(super)` | `struct StackFrame` |
//! | [`SourceFrame`] | `pub(super)` | `struct SourceFrame` |

#[allow(unused_imports, reason = "Preserve the complete façade interface")]
pub(super) use crate::ipc::stack::{SourceFrame, StackFrame};
