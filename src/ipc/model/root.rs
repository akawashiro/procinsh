//! Data values shared by acquisition and unprivileged analysis.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`memory`] | `pub(crate)` | `mod memory` |
//! | [`resource`] | `pub(crate)` | `mod resource` |
//! | [`socket_types`] | `pub(crate)` | `mod socket_types` |
//! | [`process`] | `pub(crate)` | `mod process` |
//! | [`threads`] | `pub(crate)` | `mod threads` |
//! | [`sockets`] | `pub(crate)` | `mod sockets` |
//! | [`system`] | `pub(crate)` | `mod system` |

pub(crate) mod memory;
pub(crate) mod resource;
pub(crate) mod socket_types;

pub(crate) mod process;
pub(crate) mod threads;

pub(crate) mod sockets;
pub(crate) mod system;
