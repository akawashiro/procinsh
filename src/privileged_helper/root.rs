//! Helper-private inspection modules and acquisition service.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`launch_privileged_helper`] | `pub(crate)` | `fn launch_privileged_helper() -> `[`Result`](anyhow::Result)`<()>` |
//!
//! Shared resource identities live in [`crate::ipc::model::resource`]; socket metadata and descriptor
//! capabilities live in [`crate::ipc::model::socket_types`]; bounded netlink dumps live in [`netlink`].

#[cfg(test)]
#[path = "test_http/api/root.rs"]
mod api;
mod launch;
mod privileges;
pub(crate) use launch::launch_privileged_helper;
mod helper_service;
#[cfg(test)]
#[path = "../http_server/middleware.rs"]
mod middleware;
mod netlink;
mod process;
#[cfg(test)]
#[path = "../http_server/router.rs"]
mod router;
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

mod bpf_records;
