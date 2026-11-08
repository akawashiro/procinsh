//! Shared native IPC data, codec and bounded Unix-stream framing. JSON conversion belongs to main.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`schema`] | `pub(super)` | `mod schema` |
//! | [`transport`] | `pub(super)` | `mod transport` |
//! | [`codec`] | `pub(super)` | `mod codec` |
//! | [`model`] | `pub(super)` | `mod model` |
//! | [`activity`] | `pub(super)` | `mod activity` |
//! | [`descriptors`] | `pub(super)` | `mod descriptors` |
//! | [`hardware`] | `pub(super)` | `mod hardware` |
//! | [`history`] | `pub(super)` | `mod history` |
//! | [`registers`] | `pub(super)` | `mod registers` |
//! | [`stack`] | `pub(super)` | `mod stack` |
//! | [`acquisition`] | `pub(crate)` | `mod acquisition` |
//! | [`elf`] | `pub(super)` | `mod elf` |
//! | [`rdma`] | `pub(super)` | `mod rdma` |

// Each executable uses a different subset of the shared wire declarations.
#![allow(dead_code)]

pub(super) mod schema;
pub(super) mod transport;

pub(super) mod codec;

#[path = "model/root.rs"]
pub(super) mod model;

pub(super) mod activity;
pub(super) mod descriptors;
pub(super) mod hardware;
pub(super) mod history;
pub(super) mod registers;
pub(super) mod stack;

pub(crate) mod acquisition;

pub(super) mod elf;

pub(super) mod rdma;
