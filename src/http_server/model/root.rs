//! Data values shared by acquisition and unprivileged analysis.

//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`memory`] | `pub` | `mod memory` |
//! | [`resource`] | `pub` | `mod resource` |
//! | [`socket_types`] | `pub` | `mod socket_types` |
//! | [`process`] | `pub` | `mod process` |
//! | [`threads`] | `pub` | `mod threads` |
//! | [`sockets`] | `pub` | `mod sockets` |
//! | [`system`] | `pub` | `mod system` |

pub mod memory;
pub mod resource;
pub mod socket_types;

pub mod process;
pub mod threads;

pub mod sockets;
pub mod system;
