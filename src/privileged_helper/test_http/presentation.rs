//! HTTP presentation code included only by helper contract tests.

//!
//! # Interface
//! Test-only adapters; this module is absent from normal builds.
// Contract tests exercise only part of the HTTP presentation implementation.
#![allow(dead_code, unused_imports)]

#[path = "../../http_server/analysis/root.rs"]
pub(crate) mod analysis;
#[path = "../../http_server/json.rs"]
mod json;
#[path = "../../http_server/model/root.rs"]
pub(crate) mod model;
