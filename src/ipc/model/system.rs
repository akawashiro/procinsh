//! Captured system structure and main-generated descriptor relations.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`ProcessSnapshot`] | `pub(crate)` | `struct ProcessSnapshot` |
//! | [`FdEndpoint`] | `pub(crate)` | `struct FdEndpoint` |
//! | [`SocketEndpoint`] | `pub(crate)` | `struct SocketEndpoint` |
//! | [`FdRelation`] | `pub(crate)` | `struct FdRelation` |
//! | [`SystemSnapshot`] | `pub(crate)` | `struct SystemSnapshot` |
//! | [`NeighborSnapshot`] | `pub(crate)` | `struct NeighborSnapshot` |

use crate::ipc::{
    hardware::{DeviceHandle, HardwareDevice, HardwarePath},
    model::{memory::MemoryMap, resource::IpcIdentity, socket_types::*, sockets::SocketInfo},
    schema::ProcessId,
};
/// A Linux process and its observed structural address space, without RSS/PSS.
/// CPU and RSS measurements belong to [`crate::ipc::acquisition::ProcessSummary`].
#[derive(Clone)]
pub(crate) struct ProcessSnapshot {
    pub(crate) identity: ProcessId,
    pub(crate) parent_id: Option<ProcessId>,
    pub(crate) name: String,
    /// Complete argument vector from /proc; absent when it cannot be read.
    pub(crate) command_line: Option<Vec<String>>,
    pub(crate) uid: Option<u32>,
    pub(crate) username: Option<String>,
    pub(crate) euid: Option<u32>,
    pub(crate) effective_username: Option<String>,
    pub(crate) maps: Vec<MemoryMap>,
    pub(crate) maps_epoch: u64,
    pub(crate) maps_error: Option<String>,
}
/// A process-owned file descriptor endpoint, including coalesced shared FDs.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct FdEndpoint {
    pub(crate) process_id: ProcessId,
    pub(crate) fd: u32,
    pub(crate) fd_count: usize,
    pub(crate) resource: IpcIdentity,
    pub(crate) kind: FdKind,
    pub(crate) access: FdAccess,
    /// Independent connection observation; null for non-socket resources.
    pub(crate) connection_status: Option<SocketConnectionStatus>,
}
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct SocketEndpoint {
    pub(crate) protocol: SocketProtocol,
    /// Null when no state was observed, even if the socket kind is known.
    pub(crate) state: Option<SocketState>,
    pub(crate) local: Option<InetAddress>,
    pub(crate) remote: Option<InetAddress>,
    pub(crate) path: Option<String>,
    pub(crate) network_peer: bool,
    pub(crate) remote_hostname: Option<String>,
    /// The remote InfiniBand port GID when the destination is an IPoIB neighbor.
    pub(crate) remote_gid: Option<String>,
}
impl From<&SocketInfo> for SocketEndpoint {
    fn from(info: &SocketInfo) -> Self {
        Self {
            protocol: info.protocol,
            remote_hostname: None,
            remote_gid: None,
            state: info.state,
            local: info.local.map(InetAddress::from),
            remote: info.remote.map(InetAddress::from),
            path: info.path.clone(),
            network_peer: crate::ipc::model::socket_types::socket_protocol_is_tcp_or_udp(
                info.protocol,
            ) && info.state != Some(SocketState::Listen)
                && info
                    .remote
                    .is_some_and(|remote| remote.port() != 0 && !remote.ip().is_unspecified()),
        }
    }
}
/// A socket, pipe, or shared-ownership relation between file descriptor endpoints.
/// An absent peer represents an external or unidentified process endpoint.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct FdRelation {
    pub(crate) id: String,
    pub(crate) endpoint: FdEndpoint,
    pub(crate) peer: Option<FdEndpoint>,
    pub(crate) label: String,
    pub(crate) socket: Option<SocketEndpoint>,
    pub(crate) candidate: bool,
    pub(crate) shared: bool,
}
/// A system-wide observation of processes and their file descriptor relations.
#[derive(Clone, Default)]
// Re-exported by system for subscription consumers.
pub(crate) struct SystemSnapshot {
    pub(crate) captured_at: u64,
    pub(crate) processes: Vec<ProcessSnapshot>,
    pub(crate) fd_relations: Vec<FdRelation>,
    pub(crate) rdma_ports: Vec<crate::ipc::rdma::RdmaPortInfo>,
    pub(crate) rdma_relations: Vec<crate::ipc::rdma::RdmaRelation>,
    pub(crate) hardware_devices: Vec<HardwareDevice>,
    pub(crate) hardware_paths: Vec<HardwarePath>,
    pub(crate) device_handles: Vec<DeviceHandle>,
    pub(crate) warnings: Vec<String>,
    pub(crate) inspected_processes: usize,
    pub(crate) inspected_fds: usize,
    pub(crate) raw_owners: Vec<(IpcIdentity, Vec<FdEndpoint>)>,
    pub(crate) raw_sockets: Vec<(u64, crate::ipc::model::sockets::SocketInfo)>,
    pub(crate) raw_namespaces: Vec<(u64, (u64, u64))>,
    pub(crate) raw_connections: Vec<(IpcIdentity, SocketConnectionStatus)>,
    pub(crate) raw_device_handles: Vec<(ProcessId, u32, u64)>,
    pub(crate) raw_neighbors: Option<NeighborSnapshot>,
    pub(crate) raw_hardware: crate::ipc::hardware::HardwareFiles,
    pub(crate) raw_hostnames: Vec<(std::net::IpAddr, String)>,
}

/// Neighbor observations scoped to the collector network namespace.
#[derive(Clone)]
pub(crate) struct NeighborSnapshot {
    pub(crate) entries: Vec<(std::net::IpAddr, String)>,
    pub(crate) namespace: (u64, u64),
}

crate::ipc::codec::wire_struct!(ProcessSnapshot {
    identity,
    parent_id,
    name,
    command_line,
    uid,
    username,
    euid,
    effective_username,
    maps,
    maps_epoch,
    maps_error
});

crate::ipc::codec::wire_struct!(FdEndpoint {
    process_id,
    fd,
    fd_count,
    resource,
    kind,
    access,
    connection_status
});

crate::ipc::codec::wire_struct!(SocketEndpoint {
    protocol,
    state,
    local,
    remote,
    path,
    network_peer,
    remote_hostname,
    remote_gid
});

crate::ipc::codec::wire_struct!(FdRelation {
    id,
    endpoint,
    peer,
    label,
    socket,
    candidate,
    shared
});

crate::ipc::codec::wire_struct!(SystemSnapshot {
    captured_at,
    processes,
    fd_relations,
    rdma_ports,
    rdma_relations,
    hardware_devices,
    hardware_paths,
    device_handles,
    warnings,
    inspected_processes,
    inspected_fds,
    raw_owners,
    raw_sockets,
    raw_namespaces,
    raw_connections,
    raw_device_handles,
    raw_neighbors,
    raw_hardware,
    raw_hostnames
});

crate::ipc::codec::wire_struct!(NeighborSnapshot { entries, namespace });
