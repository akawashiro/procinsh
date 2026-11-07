//! Captured system structure and main-generated descriptor relations.
//!
//! # Interface
//!
//! No named types or functions are exported; shared values come from their definition modules.

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
    pub process_id: ProcessId,
    pub fd: u32,
    pub fd_count: usize,
    pub resource: IpcIdentity,
    pub kind: FdKind,
    pub access: FdAccess,
    /// Independent connection observation; null for non-socket resources.
    pub connection_status: Option<SocketConnectionStatus>,
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
    pub id: String,
    pub endpoint: FdEndpoint,
    pub peer: Option<FdEndpoint>,
    pub label: String,
    pub socket: Option<SocketEndpoint>,
    pub candidate: bool,
    pub shared: bool,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw_owners: Vec<(IpcIdentity, Vec<FdEndpoint>)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw_sockets: Vec<(u64, crate::ipc::model::sockets::SocketInfo)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw_namespaces: Vec<(u64, (u64, u64))>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw_connections: Vec<(IpcIdentity, SocketConnectionStatus)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw_device_handles: Vec<(ProcessId, u32, u64)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_neighbors: Option<NeighborSnapshot>,
    #[serde(skip)]
    pub raw_hardware: crate::ipc::hardware::HardwareFiles,
    #[serde(skip)]
    pub raw_hostnames: Vec<(std::net::IpAddr, String)>,
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
    pub entries: Vec<(std::net::IpAddr, String)>,
    pub namespace: (u64, u64),
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
