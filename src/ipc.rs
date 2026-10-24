//! Shared IPC schema and bounded framing, without inspection implementations.
//!
//! # Interface
//! - [`schema`] defines process identities, closed requests, responses and events.
//! - [`transport`] provides bounded Unix stream read/write functions and deadlines.
pub mod schema;
pub mod transport;
