//! Data values shared by acquisition and unprivileged analysis.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`memory`] | `pub(crate)` | `mod memory` |
//! | [`resource`] | `pub(crate)` | `mod resource` |
//! | [`process`] | `pub(crate)` | `mod process` |

pub(crate) mod memory;
pub(crate) mod resource;
mod socket_types;

pub(crate) mod process;
mod threads;

mod sockets;
mod system;
