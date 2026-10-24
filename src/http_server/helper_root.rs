//! Helper-private inspection modules and acquisition service.
//!
//! # Interface
//!
//! | Definition | Visibility | Signature |
//! | --- | --- | --- |
//! | [`helper::run`] | `pub(crate)` | `fn run() -> anyhow::Result<()>` |
//!
//! Shared resource identities live in [`resource`]; socket metadata and descriptor
//! capabilities live in [`socket_types`]; bounded netlink dumps live in [`netlink`].
//!
#[cfg(test)]
mod api;
pub(crate) mod helper;
mod helper_service;
#[cfg(test)]
mod middleware;
mod netlink;
mod process;
mod resource;
#[cfg(test)]
mod router;
mod socket_types;
#[cfg(test)]
mod state;
mod system;
#[cfg(test)]
use state::AppState;
#[cfg(test)]
mod process_api_tests;
#[cfg(test)]
mod system_api_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod web;

#[cfg(test)]
mod helper_tests;

#[cfg(test)]
#[path = "../application/delivery.rs"]
mod application_delivery;
