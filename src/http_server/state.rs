//! Shared HTTP application state and observation processing lifecycle.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`AppState`] | `pub(super)` | `struct AppState` |
//! | [`remote_app_state`] | `pub(super)` | `fn remote_app_state( interval: `[`Duration`]`, client: `[`MonitorClient`]`, hosts: `[`Arc`]`<`[`HostRegistry`]`>, ) -> `[`AppState`] |
//! | [`stopped_app_state`] | `pub(super)` | `fn stopped_app_state(state: &`[`AppState`]`) -> bool` |
//! | [`stop_app_state`] | `pub(super)` | `fn stop_app_state(state: &`[`AppState`]`)` |

use super::{client::MonitorClient, hosts::HostRegistry};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

pub(super) struct AppState {
    pub(super) client: MonitorClient,
    pub(super) interval: Duration,
    pub(super) hosts: Arc<HostRegistry>,
    stopped: AtomicBool,
}
pub(super) fn remote_app_state(
    interval: Duration,
    client: MonitorClient,
    hosts: Arc<HostRegistry>,
) -> AppState {
    AppState {
        client,
        interval,
        hosts,
        stopped: AtomicBool::new(false),
    }
}

pub(super) fn stopped_app_state(state: &AppState) -> bool {
    state.stopped.load(Ordering::Relaxed)
}

pub(super) fn stop_app_state(state: &AppState) {
    state.stopped.store(true, Ordering::Relaxed);
}
