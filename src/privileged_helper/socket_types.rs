//! Shared socket types values used during acquisition.

//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`AddressFamily`] | `pub(super)` | `enum AddressFamily` |
//! | [`SocketType`] | `pub(super)` | `enum SocketType` |
//! | [`SocketProtocol`] | `pub(super)` | `enum SocketProtocol` |
//! | [`SocketConnectionStatus`] | `pub(super)` | `enum SocketConnectionStatus` |
//! | [`SocketState`] | `pub(super)` | `enum SocketState` |
//! | [`FdKind`] | `pub(super)` | `enum FdKind` |
//! | [`FdAccess`] | `pub(super)` | `enum FdAccess` |
//! | [`InetAddress`] | `pub(super)` | `struct InetAddress` |

pub(super) use crate::ipc::model::socket_types::*;
