//! Closed monitor operations and structured replies. No request accepts a path,
//! arbitrary address, syscall number, file descriptor, or caller-selected limit.
//!
//! Frames have a four-byte big-endian length. Requests are at most 1 KiB,
//! responses at most 32 MiB, and at most 64 requests may queue in the child.
//! Socket I/O and application requests have 30-second deadlines. Each side
//! treats an invalid/truncated frame or a timeout as a fatal connection error.
//! Sessions expire after 15 seconds without polling and are capped at 32 per
//! kind. Event polling never blocks waiting for a new observation.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / type |
//! | --- | --- | --- |
//! | [`ProcessResource`] | `pub(in crate::http_server)` | `enum ProcessResource` |
//! | [`MonitorRequest`] | `pub(in crate::http_server)` | `enum MonitorRequest` |
//! | [`MonitorEvent`] | `pub(in crate::http_server)` | `enum MonitorEvent` |
//! | [`MonitorResponse`] | `pub(in crate::http_server)` | `enum MonitorResponse` |
//! | [`WireResponse`] | `pub(super)` | `type WireResponse = Result<`[`MonitorResponse`]`, String>` |
//!
//! Target identities use [`ProcessId`]; system events use [`SystemSnapshot`].

use crate::http_server::{process::ProcessId, system::SystemSnapshot};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

pub(super) const MAX_REQUEST: usize = 1024;
pub(super) const MAX_RESPONSE: usize = 32 * 1024 * 1024;
pub(super) const TIMEOUT: Duration = Duration::from_secs(30);
pub(super) const SESSION_LEASE: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Deserialize, Serialize)]
pub(in crate::http_server) enum ProcessResource {
    Observation,
    Threads,
    Maps,
    Environment,
    Auxv,
    Fds,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(in crate::http_server) enum MonitorRequest {
    Processes,
    Process {
        id: ProcessId,
        resource: ProcessResource,
    },
    ObserveProcess {
        id: ProcessId,
    },
    ObserveSystem,
    Poll {
        session: u64,
    },
    Close {
        session: u64,
    },
}

#[derive(Deserialize, Serialize)]
pub(in crate::http_server) enum MonitorEvent {
    Observation(Value),
    Snapshot(SystemSnapshot),
    Activity(Value),
    Gap {
        dropped: u64,
        snapshot: SystemSnapshot,
    },
}

#[derive(Deserialize, Serialize)]
pub(in crate::http_server) enum MonitorResponse {
    Data(Value),
    Started {
        session: u64,
        initial: MonitorEvent,
    },
    Event {
        event: Option<MonitorEvent>,
        closed: bool,
    },
    Closed,
}

/// Errors are data on the wire; neither HTTP status codes nor HTTP syntax are
/// interpreted in the parent. The HTTP layer chooses the response status.
pub(super) type WireResponse = Result<MonitorResponse, String>;
