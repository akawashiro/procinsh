//! Build descriptor peer and shared-holder output from acquired observations.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`Key`] | `pub` | `struct Key` |
//! | [`Endpoint`] | `pub` | `struct Endpoint` |
//! | [`FileDescriptorInfo`] | `pub` | `struct FileDescriptorInfo` |
//! | [`FileDescriptors`] | `pub` | `struct FileDescriptors` |
//! | [`Owner`] | `pub` | `struct Owner` |
//! | [`resolve_descriptor_relations`] | `pub` | `fn resolve_descriptor_relations(data: &mut FileDescriptors)` |

pub use crate::ipc::descriptors::{Endpoint, FileDescriptorInfo, FileDescriptors, Key, Owner};
use crate::{http_server::model::socket_types::*, ipc::schema::ProcessId};
use std::collections::HashMap;
#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::descriptors::Key")]
struct KeyJson {
    pub kind: FdKind,
    pub device: u64,
    pub inode: u64,
}
impl serde::Serialize for crate::ipc::descriptors::Key {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        KeyJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::descriptors::Key {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        KeyJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::descriptors::Endpoint")]
struct EndpointJson {
    pub process_id: ProcessId,
    pub name: String,
    pub fd: u32,
    pub access: FdAccess,
    pub relation: String,
}
impl serde::Serialize for crate::ipc::descriptors::Endpoint {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        EndpointJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::descriptors::Endpoint {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        EndpointJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::descriptors::FileDescriptorInfo")]
struct FileDescriptorInfoJson {
    pub fd: u32,
    pub kind: FdKind,
    pub inode: String,
    pub target: String,
    pub access: FdAccess,
    pub protocol: Option<SocketProtocol>,
    pub state: Option<SocketState>,
    /// Socket listener/peer availability; null for non-socket descriptors.
    pub connection_status: Option<SocketConnectionStatus>,
    pub local: Option<InetAddress>,
    pub remote: Option<InetAddress>,
    pub path: Option<String>,
    pub peer_inode: Option<String>,
    pub peers: Vec<Endpoint>,
    pub holders: Vec<Endpoint>,
    pub note: String,
    pub key: Key,
    pub mode: Option<u32>,
    pub socket_device: Option<u64>,
}
impl serde::Serialize for crate::ipc::descriptors::FileDescriptorInfo {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        FileDescriptorInfoJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::descriptors::FileDescriptorInfo {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        FileDescriptorInfoJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::descriptors::FileDescriptors")]
struct FileDescriptorsJson {
    pub process_id: ProcessId,
    pub captured_at: u64,
    pub entries: Vec<FileDescriptorInfo>,
    pub warnings: Vec<String>,
    #[serde(default)]
    pub raw_fd_owners: Vec<(Key, Vec<Owner>)>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub raw_peer_keys: Vec<(u32, Vec<Key>)>,
}
impl serde::Serialize for crate::ipc::descriptors::FileDescriptors {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        FileDescriptorsJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::descriptors::FileDescriptors {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        FileDescriptorsJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::descriptors::Owner")]
struct OwnerJson {
    pub endpoint: Endpoint,
    pub mode: Option<u32>,
}
impl serde::Serialize for crate::ipc::descriptors::Owner {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        OwnerJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::descriptors::Owner {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        OwnerJson::deserialize(d)
    }
}

fn pipe_opposite(a: Option<u32>, b: Option<u32>) -> bool {
    matches!(
        (a, b),
        (Some(0), Some(1 | 2)) | (Some(1), Some(0 | 2)) | (Some(2), Some(0..=2))
    )
}

pub fn resolve_descriptor_relations(data: &mut FileDescriptors) {
    let id = data.process_id;
    let owners: HashMap<_, _> = std::mem::take(&mut data.raw_fd_owners)
        .into_iter()
        .collect();
    let peer_keys: HashMap<_, _> = std::mem::take(&mut data.raw_peer_keys)
        .into_iter()
        .collect();
    let entries = &mut data.entries;
    for entry in entries {
        if let Some(found) = owners.get(&entry.key) {
            for owner in found {
                if owner.endpoint.process_id == id && owner.endpoint.fd == entry.fd {
                    continue;
                }
                let mut endpoint = owner.endpoint.clone();
                if entry.kind != FdKind::Socket && pipe_opposite(entry.mode, owner.mode) {
                    endpoint.relation = format!("{} end of the same pipe", endpoint.access);
                    entry.peers.push(endpoint);
                } else {
                    endpoint.relation = if entry.kind == FdKind::Socket {
                        "Holder of the same socket"
                    } else {
                        "Same pipe, same or unknown direction"
                    }
                    .into();
                    entry.holders.push(endpoint);
                }
                if entry.peers.len() + entry.holders.len() >= 64 {
                    entry.note = "Showing up to 64 peer/shared FDs.".into();
                    break;
                }
            }
        }
        for key in peer_keys.get(&entry.fd).into_iter().flatten() {
            if let Some(found) = owners.get(key) {
                for owner in found {
                    if entry.peers.len() >= 64 {
                        entry.note = "Showing up to 64 peer FDs.".into();
                        break;
                    }
                    let mut endpoint = owner.endpoint.clone();
                    endpoint.relation = if entry.peer_inode.is_some() {
                        "UNIX peer"
                    } else {
                        "Reversed address/port match (candidate)"
                    }
                    .into();
                    entry.peers.push(endpoint);
                }
            }
        }
        if entry.peers.is_empty() && entry.note.is_empty() {
            entry.note = if entry.state == Some(SocketState::Listen) {
                "Listening (no peer PID)"
            } else {
                "Peer PID unknown (remote, unconnected, exited, unobserved, or access denied)"
            }
            .into();
        }
    }
}
