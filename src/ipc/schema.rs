//! Closed monitor operations and structured replies. No request accepts a path,
//! arbitrary process-memory read, syscall number, file descriptor, or caller-selected limit.
//! Mapped ELF requests select an existing executable mapping by its start address;
//! the helper independently verifies the process, mapped file identity and file ranges.
//!
//! Frames have a four-byte native-endian length and a native binary payload.
//! Requests are at most 1 KiB,
//! responses at most 1 GiB, and at most 64 requests may queue in the application.
//! Socket I/O and application requests have 30-second deadlines. Each side
//! treats an invalid/truncated frame or a timeout as a fatal connection error.
//! Sessions expire after 15 seconds without polling and are capped at 32 per
//! kind. Event polling never blocks waiting for a new observation.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`MAX_REQUEST`] | `pub(crate)` | `const MAX_REQUEST: usize` |
//! | [`MAX_RESPONSE`] | `pub(crate)` | `const MAX_RESPONSE: usize` |
//! | [`TIMEOUT`] | `pub(crate)` | `const TIMEOUT: Duration` |
//! | [`SESSION_LEASE`] | `pub(crate)` | `const SESSION_LEASE: Duration` |
//! | [`ProcessResource`] | `pub(crate)` | `enum ProcessResource` |
//! | [`MonitorRequest`] | `pub(crate)` | `enum MonitorRequest` |
//! | [`MonitorEvent`] | `pub(crate)` | `enum MonitorEvent` |
//! | [`MonitorResponse`] | `pub(crate)` | `enum MonitorResponse` |
//! | [`WireResponse`] | `pub(crate)` | `type WireResponse` |
//! | [`ProcessId`] | `pub(crate)` | `struct ProcessId` |
//! | [`HelperIdentity`] | `pub(crate)` | `struct HelperIdentity` |
//! | [`AcquiredData`] | `pub(crate)` | `enum AcquiredData` |
//!
//! Target identities use [`ProcessId`]. Acquisition snapshots use

use std::time::Duration;

pub(crate) const MAX_REQUEST: usize = 1024;
pub(crate) const MAX_RESPONSE: usize = 1024 * 1024 * 1024;
pub(crate) const TIMEOUT: Duration = Duration::from_secs(30);
pub(crate) const SESSION_LEASE: Duration = Duration::from_secs(15);

#[derive(Clone, Copy)]
pub(crate) enum ProcessResource {
    Observation,
    Threads,
    Maps,
    Environment,
    Auxv,
    Fds,
}

pub(crate) enum MonitorRequest {
    Processes,
    Process {
        id: ProcessId,
        resource: ProcessResource,
    },
    MappedElf {
        id: ProcessId,
        mapping_start: u64,
        ranges: Vec<crate::ipc::elf::FileRange>,
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

pub(crate) enum MonitorEvent {
    Observation(std::sync::Arc<crate::ipc::acquisition::ProcessDetailSnapshot>),
    AcquiredSnapshot(std::sync::Arc<crate::ipc::model::system::SystemSnapshot>),
    AcquisitionGap {
        dropped: u64,
        snapshot: std::sync::Arc<crate::ipc::model::system::SystemSnapshot>,
    },
    Activity(std::sync::Arc<crate::ipc::acquisition::SystemActivity>),
}

pub(crate) enum MonitorResponse {
    Data(AcquiredData),
    /// Raw ELF bytes in the native binary payload.
    Elf(Option<crate::ipc::elf::MappedFileData>),
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
pub(crate) type WireResponse = Result<MonitorResponse, String>;

/// PID plus kernel start time, independently validated by the helper.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ProcessId {
    pub(crate) pid: i32,
    pub(crate) start_time_ticks: u64,
}

/// Startup handshake tied to the child returned by Command::spawn.
pub(crate) struct HelperIdentity {
    pub(crate) pid: u32,
    pub(crate) capabilities: u64,
}

crate::ipc::codec::wire_struct!(ProcessId {
    pid,
    start_time_ticks
});
crate::ipc::codec::wire_struct!(HelperIdentity { pid, capabilities });
crate::ipc::codec::wire_enum!(ProcessResource {0=>Observation,1=>Threads,2=>Maps,3=>Environment,4=>Auxv,5=>Fds});
crate::ipc::codec::wire_enum!(MonitorRequest {0=>Processes,1=>Process{id,resource},2=>MappedElf{id,mapping_start,ranges},3=>ObserveProcess{id},4=>ObserveSystem,5=>Poll{session},6=>Close{session}});
crate::ipc::codec::wire_enum!(MonitorEvent {0=>Observation(value),1=>AcquiredSnapshot(value),2=>AcquisitionGap{dropped,snapshot},3=>Activity(value)});
crate::ipc::codec::wire_enum!(MonitorResponse {0=>Data(value),1=>Elf(value),3=>Started{session,initial},4=>Event{event,closed},5=>Closed});
pub(crate) enum AcquiredData {
    Processes(Vec<crate::ipc::acquisition::ProcessSummary>),
    Observation(crate::ipc::acquisition::ObservationData),
    Maps(crate::ipc::acquisition::MemoryMaps),
    Environment(crate::ipc::acquisition::EnvironmentData),
    Auxv(crate::ipc::acquisition::AuxData),
    Fds(crate::ipc::descriptors::FileDescriptors),
}
crate::ipc::codec::wire_enum!(AcquiredData {0=>Processes(value),1=>Observation(value),2=>Maps(value),3=>Environment(value),4=>Auxv(value),5=>Fds(value)});
