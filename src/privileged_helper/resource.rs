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

pub(super) use crate::ipc::model::resource::*;
