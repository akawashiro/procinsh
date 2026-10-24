//! Closed monitor operations and structured replies. No request accepts a path,
//! arbitrary address, syscall number, file descriptor, or caller-selected limit.
//!
//! Frames have a four-byte big-endian length. Requests are at most 1 KiB,
//! responses at most 32 MiB, and at most 64 requests may queue in the application.
//! Socket I/O and application requests have 30-second deadlines. Each side
//! treats an invalid/truncated frame or a timeout as a fatal connection error.
//! Sessions expire after 15 seconds without polling and are capped at 32 per
//! kind. Event polling never blocks waiting for a new observation.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / type |
//! | --- | --- | --- |
//! | [`ProcessId`] | `pub` | `struct ProcessId` |
//! | [`HelperIdentity`] | `pub` | `struct HelperIdentity` |
//! | [`ProcessResource`] | `pub` | `enum ProcessResource` |
//! | [`MonitorRequest`] | `pub` | `enum MonitorRequest` |
//! | [`MonitorEvent`] | `pub` | `enum MonitorEvent` |
//! | [`MonitorResponse`] | `pub` | `enum MonitorResponse` |
//! | [`WireResponse`] | `pub` | `type WireResponse = Result<`[`MonitorResponse`]`, String>` |
//!
//! Target identities use [`ProcessId`]; system events use [`Value`].

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

pub const MAX_REQUEST: usize = 1024;
pub const MAX_RESPONSE: usize = 32 * 1024 * 1024;
pub const TIMEOUT: Duration = Duration::from_secs(30);
pub const SESSION_LEASE: Duration = Duration::from_secs(15);

#[derive(Clone, Copy, Deserialize, Serialize)]
pub enum ProcessResource {
    Observation,
    Threads,
    Maps,
    Environment,
    Auxv,
    Fds,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub enum MonitorRequest {
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
pub enum MonitorEvent {
    Observation(Value),
    Snapshot(Value),
    Activity(Value),
    Gap { dropped: u64, snapshot: Value },
}

#[derive(Deserialize, Serialize)]
pub enum MonitorResponse {
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
/// interpreted in the helper. The HTTP layer chooses the response status.
pub type WireResponse = Result<MonitorResponse, String>;

/// PID plus kernel start time, independently validated by the helper.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(deny_unknown_fields)]
pub struct ProcessId {
    pub pid: i32,
    pub start_time_ticks: u64,
}

/// Startup handshake tied to the child returned by Command::spawn.
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HelperIdentity {
    pub pid: u32,
    pub version: String,
    pub capabilities: u64,
}
