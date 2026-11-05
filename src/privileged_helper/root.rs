//! Helper-private inspection modules and acquisition service.
//!
//! # Interface
//!
//! | Definition | Visibility | Signature |
//! | --- | --- | --- |
//! | [`run`] | `pub(crate)` | `fn run() -> anyhow::Result<()>` |
//! | [`bpf_records`] | `pub(crate)` | `mod bpf_records` |
//!
//! Shared resource identities live in [`resource`]; socket metadata and descriptor
//! capabilities live in [`socket_types`]; bounded netlink dumps live in [`netlink`].
//!
#[cfg(test)]
#[path = "test_http/api/root.rs"]
mod api;
mod launch;
mod privileges;
pub(crate) use launch::run;
mod helper_service;
#[cfg(test)]
#[path = "../http_server/middleware.rs"]
mod middleware;
mod netlink;
mod process;
mod resource;
#[cfg(test)]
#[path = "../http_server/router.rs"]
mod router;
mod socket_types;
#[cfg(test)]
#[path = "test_http/state.rs"]
mod state;
mod system;
#[cfg(test)]
use state::AppState;
#[cfg(test)]
#[path = "test_http/process_api_tests.rs"]
mod process_api_tests;
#[cfg(test)]
#[path = "test_http/system_api_tests.rs"]
mod system_api_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
#[path = "test_http/tests.rs"]
mod tests;
#[cfg(test)]
#[path = "../http_server/web.rs"]
mod web;

#[cfg(test)]
mod helper_tests;

#[cfg(test)]
#[path = "../http_server/delivery.rs"]
mod http_delivery;

pub(crate) mod bpf_records;
