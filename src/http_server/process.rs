//! Process façade for HTTP transport and system monitoring.
mod details;
mod discovery;
mod fds;
mod identity;
mod maps;
mod memory;
mod monitoring;
mod procfs;
mod resources;
mod signals;
mod snapshot;
mod sockets;
mod threads;
pub use discovery::{Discovery, ProcessSummary};
use discovery::{summary, users};
#[cfg(test)]
pub use identity::identity;
pub use identity::{ProcessId, check_identity};
pub use maps::MemoryMap;
pub use monitoring::Monitoring;
use resources::permission_help;
pub use resources::{
    MAX_READ, Snapshotter, SubscribeError, auxv, environment, fds, fields, inet_sockets, maps,
    memory, memory_maps, observation, signals, socket_text, threads, ticks_per_second,
    timestamp_ms, unix_socket_peers, unix_sockets,
};
pub use sockets::SocketInfo;
#[cfg(test)]
mod test_support;
#[cfg(test)]
pub use test_support::Target as TestTarget;
#[cfg(test)]
mod tests;
