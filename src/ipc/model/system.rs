//! Captured system structure and main-generated descriptor relations.

//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`ProcessSnapshot`] | `pub` | `struct ProcessSnapshot` |
//! | [`FdEndpoint`] | `pub` | `struct FdEndpoint` |
//! | [`SocketEndpoint`] | `pub` | `struct SocketEndpoint` |
//! | [`FdRelation`] | `pub` | `struct FdRelation` |
//! | [`SystemSnapshot`] | `pub` | `struct SystemSnapshot` |
//! | [`NeighborSnapshot`] | `pub` | `struct NeighborSnapshot` |

use crate::{
    ipc::hardware::{DeviceHandle, HardwareDevice, HardwarePath},
    ipc::model::{memory::MemoryMap, resource::IpcIdentity, socket_types::*, sockets::SocketInfo},
    ipc::schema::ProcessId,
};
#[derive(Clone)]
pub struct ProcessSnapshot {
    pub identity: ProcessId,
    pub parent_id: Option<ProcessId>,
    pub name: String,
    /// Complete argument vector from /proc; absent when it cannot be read.
    pub command_line: Option<Vec<String>>,
    pub uid: Option<u32>,
    pub username: Option<String>,
    pub euid: Option<u32>,
    pub effective_username: Option<String>,
    pub maps: Vec<MemoryMap>,
    pub maps_epoch: u64,
    pub maps_error: Option<String>,
}
/// A process-owned file descriptor endpoint, including coalesced shared FDs.
#[derive(Clone, PartialEq, Eq)]
pub struct FdEndpoint {
    pub process_id: ProcessId,
    pub fd: u32,
    pub fd_count: usize,
    pub resource: IpcIdentity,
    pub kind: FdKind,
    pub access: FdAccess,
    /// Independent connection observation; null for non-socket resources.
    pub connection_status: Option<SocketConnectionStatus>,
}
#[derive(Clone, PartialEq, Eq)]
pub struct SocketEndpoint {
    pub protocol: SocketProtocol,
    /// Null when no state was observed, even if the socket kind is known.
    pub state: Option<SocketState>,
    pub local: Option<InetAddress>,
    pub remote: Option<InetAddress>,
    pub path: Option<String>,
    pub network_peer: bool,
    pub remote_hostname: Option<String>,
    /// The remote InfiniBand port GID when the destination is an IPoIB neighbor.
    pub remote_gid: Option<String>,
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
            network_peer: info.protocol.is_tcp_or_udp()
                && info.state != Some(SocketState::Listen)
                && info
                    .remote
                    .is_some_and(|remote| remote.port() != 0 && !remote.ip().is_unspecified()),
        }
    }
}
/// A socket, pipe, or shared-ownership relation between file descriptor endpoints.
/// An absent peer represents an external or unidentified process endpoint.
#[derive(Clone, PartialEq, Eq)]
pub struct FdRelation {
    pub id: String,
    pub endpoint: FdEndpoint,
    pub peer: Option<FdEndpoint>,
    pub label: String,
    pub socket: Option<SocketEndpoint>,
    pub candidate: bool,
    pub shared: bool,
}
/// A system-wide observation of processes and their file descriptor relations.
#[derive(Clone, Default)]
// Re-exported by system for subscription consumers.
pub struct SystemSnapshot {
    pub captured_at: u64,
    pub processes: Vec<ProcessSnapshot>,
    pub fd_relations: Vec<FdRelation>,
    pub rdma_ports: Vec<crate::ipc::rdma::RdmaPortInfo>,
    pub rdma_relations: Vec<crate::ipc::rdma::RdmaRelation>,
    pub hardware_devices: Vec<HardwareDevice>,
    pub hardware_paths: Vec<HardwarePath>,
    pub device_handles: Vec<DeviceHandle>,
    pub warnings: Vec<String>,
    pub inspected_processes: usize,
    pub inspected_fds: usize,
    pub raw_owners: Vec<(IpcIdentity, Vec<FdEndpoint>)>,
    pub raw_sockets: Vec<(u64, crate::ipc::model::sockets::SocketInfo)>,
    pub raw_namespaces: Vec<(u64, (u64, u64))>,
    pub raw_connections: Vec<(IpcIdentity, SocketConnectionStatus)>,
    pub raw_device_handles: Vec<(ProcessId, u32, u64)>,
    pub raw_neighbors: Option<NeighborSnapshot>,
    pub raw_hardware: crate::ipc::hardware::HardwareFiles,
    pub raw_hostnames: Vec<(std::net::IpAddr, String)>,
}

/// Neighbor observations scoped to the collector network namespace.
#[derive(Clone)]
pub struct NeighborSnapshot {
    pub entries: Vec<(std::net::IpAddr, String)>,
    pub namespace: (u64, u64),
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
