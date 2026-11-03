//! Shared resource values used during acquisition.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`DeviceId`] | `pub(super)` | `struct DeviceId` |
//! | [`IpcKind`] | `pub(super)` | `enum IpcKind` |
//! | [`IpcIdentity`] | `pub(super)` | `struct IpcIdentity` |
//! | [`FileIdentity`] | `pub(super)` | `struct FileIdentity` |

#[allow(unused_imports, reason = "Preserve the complete façade interface")]
pub(super) use crate::ipc::model::resource::{DeviceId, FileIdentity, IpcIdentity, IpcKind};
