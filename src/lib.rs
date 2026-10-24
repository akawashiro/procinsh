//! Structured protocol shared by the application and its separately executed helper.
//!
//! # Interface
//! [`protocol`] defines closed requests, responses, events, and process identities.
//! [`transport`] provides bounded Unix stream framing; it never transfers descriptors.
pub mod protocol;
pub mod transport;
