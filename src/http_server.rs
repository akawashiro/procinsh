//! HTTP server wiring and shared API.
mod api;
mod middleware;
mod process;
mod router;
mod server;
mod state;
mod system_monitoring;
mod web;
pub(super) use server::run;
use state::AppState;
#[cfg(test)]
mod process_api_tests;
#[cfg(test)]
mod system_api_tests;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
