//! Captured system structure and main-generated descriptor relations.
//!
//! # Interface
//!
//! No named types or functions are exported.

use crate::ipc::model::system::{
    FdEndpoint, FdRelation, NeighborSnapshot, ProcessSnapshot, SocketEndpoint,
};
use crate::ipc::{
    hardware::{DeviceHandle, HardwareDevice, HardwarePath},
    model::{
        memory::MemoryMap,
        resource::IpcIdentity,
        socket_types::{
            FdAccess, FdKind, InetAddress, SocketConnectionStatus, SocketProtocol, SocketState,
        },
    },
    schema::ProcessId,
};
#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::model::system::ProcessSnapshot")]
struct ProcessSnapshotJson {
    identity: ProcessId,
    parent_id: Option<ProcessId>,
    name: String,
    /// Complete argument vector from /proc; absent when it cannot be read.
    command_line: Option<Vec<String>>,
    uid: Option<u32>,
    username: Option<String>,
    euid: Option<u32>,
    effective_username: Option<String>,
    maps: Vec<MemoryMap>,
    maps_epoch: u64,
    maps_error: Option<String>,
}
impl serde::Serialize for crate::ipc::model::system::ProcessSnapshot {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        ProcessSnapshotJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::model::system::ProcessSnapshot {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        ProcessSnapshotJson::deserialize(d)
    }
}

/// A process-owned file descriptor endpoint, including coalesced shared FDs.
#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::model::system::FdEndpoint")]
struct FdEndpointJson {
    process_id: ProcessId,
    fd: u32,
    fd_count: usize,
    resource: IpcIdentity,
    kind: FdKind,
    access: FdAccess,
    /// Independent connection observation; null for non-socket resources.
    connection_status: Option<SocketConnectionStatus>,
}
impl serde::Serialize for crate::ipc::model::system::FdEndpoint {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        FdEndpointJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::model::system::FdEndpoint {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        FdEndpointJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::model::system::SocketEndpoint")]
struct SocketEndpointJson {
    protocol: SocketProtocol,
    /// Null when no state was observed, even if the socket kind is known.
    state: Option<SocketState>,
    local: Option<InetAddress>,
    remote: Option<InetAddress>,
    path: Option<String>,
    network_peer: bool,
    remote_hostname: Option<String>,
    /// The remote InfiniBand port GID when the destination is an IPoIB neighbor.
    remote_gid: Option<String>,
}
impl serde::Serialize for crate::ipc::model::system::SocketEndpoint {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        SocketEndpointJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::model::system::SocketEndpoint {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        SocketEndpointJson::deserialize(d)
    }
}

/// A socket, pipe, or shared-ownership relation between file descriptor endpoints.
/// An absent peer represents an external or unidentified process endpoint.
#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::model::system::FdRelation")]
struct FdRelationJson {
    id: String,
    endpoint: FdEndpoint,
    peer: Option<FdEndpoint>,
    label: String,
    socket: Option<SocketEndpoint>,
    candidate: bool,
    shared: bool,
}
impl serde::Serialize for crate::ipc::model::system::FdRelation {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        FdRelationJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::model::system::FdRelation {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        FdRelationJson::deserialize(d)
    }
}

/// A system-wide observation of processes and their file descriptor relations.
#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::model::system::SystemSnapshot")]
// Re-exported by system for subscription consumers.
struct SystemSnapshotJson {
    captured_at: u64,
    processes: Vec<ProcessSnapshot>,
    fd_relations: Vec<FdRelation>,
    rdma_ports: Vec<crate::ipc::rdma::RdmaPortInfo>,
    rdma_relations: Vec<crate::ipc::rdma::RdmaRelation>,
    hardware_devices: Vec<HardwareDevice>,
    hardware_paths: Vec<HardwarePath>,
    device_handles: Vec<DeviceHandle>,
    warnings: Vec<String>,
    inspected_processes: usize,
    inspected_fds: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    raw_owners: Vec<(IpcIdentity, Vec<FdEndpoint>)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    raw_sockets: Vec<(u64, crate::ipc::model::sockets::SocketInfo)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    raw_namespaces: Vec<(u64, (u64, u64))>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    raw_connections: Vec<(IpcIdentity, SocketConnectionStatus)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    raw_device_handles: Vec<(ProcessId, u32, u64)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    raw_neighbors: Option<NeighborSnapshot>,
    #[serde(skip)]
    raw_hardware: crate::ipc::hardware::HardwareFiles,
    #[serde(skip)]
    raw_hostnames: Vec<(std::net::IpAddr, String)>,
}
impl serde::Serialize for crate::ipc::model::system::SystemSnapshot {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        SystemSnapshotJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::model::system::SystemSnapshot {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        SystemSnapshotJson::deserialize(d)
    }
}

/// Neighbor observations scoped to the collector network namespace.
#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::model::system::NeighborSnapshot")]
struct NeighborSnapshotJson {
    entries: Vec<(std::net::IpAddr, String)>,
    namespace: (u64, u64),
}
impl serde::Serialize for crate::ipc::model::system::NeighborSnapshot {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        NeighborSnapshotJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::model::system::NeighborSnapshot {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        NeighborSnapshotJson::deserialize(d)
    }
}
