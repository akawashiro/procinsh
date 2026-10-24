//! Shared data-only IPC for the HTTP server and privileged helper.
//!
//! # Interface
//! [`ipc`] exposes the validated message schema and bounded Unix stream transport.
pub mod ipc;
