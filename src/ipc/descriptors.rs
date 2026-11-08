//! Native file descriptor observations, resource identities, peers and holders for IPC.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`Owner`] | `pub(crate)` | `struct Owner` |
//! | [`FileDescriptors`] | `pub(crate)` | `struct FileDescriptors` |
//! | [`FileDescriptorInfo`] | `pub(crate)` | `struct FileDescriptorInfo` |
//! | [`Endpoint`] | `pub(crate)` | `struct Endpoint` |
//! | [`Key`] | `pub(crate)` | `struct Key` |

use crate::ipc::{model::socket_types::*, schema::ProcessId};

#[derive(Debug)]
pub(crate) struct Owner {
    pub(crate) endpoint: Endpoint,
    pub(crate) mode: Option<u32>,
}

#[derive(Debug)]
pub(crate) struct FileDescriptors {
    pub(crate) process_id: ProcessId,
    pub(crate) captured_at: u64,
    pub(crate) entries: Vec<FileDescriptorInfo>,
    pub(crate) warnings: Vec<String>,
    pub(crate) raw_fd_owners: Vec<(Key, Vec<Owner>)>,
    pub(crate) raw_peer_keys: Vec<(u32, Vec<Key>)>,
}

/// Metadata for one file descriptor, including its resource, peers and shared holders.
/// `fd` is the descriptor number within the owning process.
#[derive(Debug)]
pub(crate) struct FileDescriptorInfo {
    pub(crate) fd: u32,
    pub(crate) kind: FdKind,
    /// Native inode number, retained without decimal-string conversion.
    pub(crate) inode: u64,
    pub(crate) target: String,
    pub(crate) access: FdAccess,
    pub(crate) protocol: Option<SocketProtocol>,
    pub(crate) state: Option<SocketState>,
    /// Socket listener/peer availability; null for non-socket descriptors.
    pub(crate) connection_status: Option<SocketConnectionStatus>,
    pub(crate) local: Option<InetAddress>,
    pub(crate) remote: Option<InetAddress>,
    pub(crate) path: Option<String>,
    /// Peer inode number when the socket peer was observed.
    pub(crate) peer_inode: Option<u64>,
    pub(crate) peers: Vec<Endpoint>,
    pub(crate) holders: Vec<Endpoint>,
    pub(crate) note: String,
    pub(crate) key: Key,
    pub(crate) mode: Option<u32>,
    pub(crate) socket_device: Option<u64>,
}

#[derive(Clone, Debug)]
pub(crate) struct Endpoint {
    pub(crate) process_id: ProcessId,
    pub(crate) name: String,
    pub(crate) fd: u32,
    pub(crate) access: FdAccess,
    pub(crate) relation: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct Key {
    pub(crate) kind: FdKind,
    pub(crate) device: u64,
    pub(crate) inode: u64,
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
