//! Native file descriptor observations, resource identities, peers and holders for IPC.
//!
//! # Interface
//! - [`FileDescriptorInfo`] (`pub`): `struct FileDescriptorInfo`.
//! - [`FileDescriptors`] (`pub`): `struct FileDescriptors`.
//! - [`Endpoint`] (`pub`): `struct Endpoint`.
//! - [`Key`] (`pub`): `struct Key`.
//! - [`Owner`] (`pub`): `struct Owner`.

use crate::ipc::{model::socket_types::*, schema::ProcessId};

#[derive(Debug)]
pub struct Owner {
    pub endpoint: Endpoint,
    pub mode: Option<u32>,
}

#[derive(Debug)]
pub struct FileDescriptors {
    pub process_id: ProcessId,
    pub captured_at: u64,
    pub entries: Vec<FileDescriptorInfo>,
    pub warnings: Vec<String>,
    pub raw_fd_owners: Vec<(Key, Vec<Owner>)>,
    pub raw_peer_keys: Vec<(u32, Vec<Key>)>,
}

/// Metadata for one file descriptor, including its resource, peers and shared holders.
/// `fd` is the descriptor number within the owning process.
#[derive(Debug)]
pub struct FileDescriptorInfo {
    pub fd: u32,
    pub kind: FdKind,
    pub inode: u64,
    pub target: String,
    pub access: FdAccess,
    pub protocol: Option<SocketProtocol>,
    pub state: Option<SocketState>,
    /// Socket listener/peer availability; null for non-socket descriptors.
    pub connection_status: Option<SocketConnectionStatus>,
    pub local: Option<InetAddress>,
    pub remote: Option<InetAddress>,
    pub path: Option<String>,
    pub peer_inode: Option<u64>,
    pub peers: Vec<Endpoint>,
    pub holders: Vec<Endpoint>,
    pub note: String,
    pub key: Key,
    pub mode: Option<u32>,
    pub socket_device: Option<u64>,
}

#[derive(Clone, Debug)]
pub struct Endpoint {
    pub process_id: ProcessId,
    pub name: String,
    pub fd: u32,
    pub access: FdAccess,
    pub relation: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub kind: FdKind,
    pub device: u64,
    pub inode: u64,
}

crate::ipc::codec::wire_struct!(Owner { endpoint, mode });

crate::ipc::codec::wire_struct!(FileDescriptors {
    process_id,
    captured_at,
    entries,
    warnings,
    raw_fd_owners,
    raw_peer_keys
});

crate::ipc::codec::wire_struct!(FileDescriptorInfo {
    fd,
    kind,
    inode,
    target,
    access,
    protocol,
    state,
    connection_status,
    local,
    remote,
    path,
    peer_inode,
    peers,
    holders,
    note,
    key,
    mode,
    socket_device
});

crate::ipc::codec::wire_struct!(Endpoint {
    process_id,
    name,
    fd,
    access,
    relation
});

crate::ipc::codec::wire_struct!(Key {
    kind,
    device,
    inode
});
