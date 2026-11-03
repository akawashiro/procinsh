//! Shared native IPC data, codec and bounded Unix-stream framing. JSON conversion belongs to main.

//!
//! # Interface
//! - [`schema`] (`pub(crate)`): `mod schema`.
//! - [`transport`] (`pub(crate)`): `mod transport`.
//! - [`codec`] (`pub(crate)`): `mod codec`.
//! - [`model`] (`pub(crate)`): `mod model`.
//! - [`activity`] (`pub(crate)`): `mod activity`.
//! - [`descriptors`] (`pub(crate)`): `mod descriptors`.
//! - [`hardware`] (`pub(crate)`): `mod hardware`.
//! - [`history`] (`pub(crate)`): `mod history`.
//! - [`registers`] (`pub(crate)`): `mod registers`.
//! - [`stack`] (`pub(crate)`): `mod stack`.
//! - [`acquisition`] (`pub(crate)`): `mod acquisition`.
//! - [`rdma`] (`pub(crate)`): `mod rdma`.
//! - [`elf`] (`pub(crate)`): `mod elf`.
// Each executable uses a different subset of the shared wire declarations.
#![allow(dead_code)]

pub(crate) mod schema;
pub(crate) mod transport;

pub(crate) mod codec;

#[path = "model/root.rs"]
pub(crate) mod model;

pub(crate) mod activity;
pub(crate) mod descriptors;
pub(crate) mod hardware;
pub(crate) mod history;
pub(crate) mod registers;
pub(crate) mod stack;

pub(crate) mod acquisition;

pub(crate) mod elf;

pub(crate) mod rdma;
