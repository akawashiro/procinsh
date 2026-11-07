//! Build descriptor peer and shared-holder output from acquired observations.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`resolve_descriptor_relations`] | `pub` | `fn resolve_descriptor_relations(data: &mut `[`FileDescriptors`]`)` |

use crate::ipc::descriptors::{Endpoint, FileDescriptorInfo, FileDescriptors, Key, Owner};
use crate::ipc::{
    model::socket_types::{
        FdAccess, FdKind, InetAddress, SocketConnectionStatus, SocketProtocol, SocketState,
    },
    schema::ProcessId,
};
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

// Preserve decimal strings in JSON while retaining native inode values in IPC.
fn serialize_optional_inode<S: serde::Serializer>(
    value: &Option<u64>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match value {
        Some(inode) => serializer.collect_str(inode),
        None => serializer.serialize_none(),
    }
}

fn deserialize_optional_inode<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<u64>, D::Error> {
    <Option<String> as serde::Deserialize>::deserialize(deserializer)?
        .map(|value| value.parse().map_err(serde::de::Error::custom))
        .transpose()
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::descriptors::FileDescriptorInfo")]
struct FileDescriptorInfoJson {
    pub fd: u32,
    pub kind: FdKind,
    #[serde(
        serialize_with = "crate::http_server::model::resource::decimal",
        deserialize_with = "crate::http_server::model::resource::deserialize_decimal"
    )]
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
    #[serde(
        default,
        serialize_with = "serialize_optional_inode",
        deserialize_with = "deserialize_optional_inode"
    )]
    pub peer_inode: Option<u64>,
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

#[cfg(test)]
mod inode_tests {
    use super::*;
    use crate::ipc::descriptors::{FileDescriptorInfo, Key};

    fn descriptor_with_inodes(inode: u64, peer_inode: Option<u64>) -> FileDescriptorInfo {
        FileDescriptorInfo {
            fd: 3,
            kind: FdKind::Pipe,
            inode,
            target: format!("pipe:[{inode}]"),
            access: FdAccess::Read,
            protocol: None,
            state: None,
            connection_status: None,
            local: None,
            remote: None,
            path: None,
            peer_inode,
            peers: Vec::new(),
            holders: Vec::new(),
            note: String::new(),
            key: Key {
                kind: FdKind::Pipe,
                device: 0,
                inode,
            },
            mode: Some(0),
            socket_device: None,
        }
    }

    #[test]
    fn native_inodes_preserve_decimal_json_and_ipc_precision() {
        for inode in [0, (1u64 << 53) + 1, u64::MAX] {
            for peer_inode in [None, Some(inode)] {
                let descriptor = descriptor_with_inodes(inode, peer_inode);
                let json = serde_json::to_value(&descriptor).unwrap();
                assert_eq!(json["inode"], inode.to_string());
                assert_eq!(
                    json["peer_inode"],
                    peer_inode.map_or(serde_json::Value::Null, |value| value.to_string().into())
                );
                let parsed: FileDescriptorInfo = serde_json::from_value(json).unwrap();
                assert_eq!((parsed.inode, parsed.peer_inode), (inode, peer_inode));
                let bytes = crate::ipc::codec::encode(&descriptor, 4096).unwrap();
                let decoded: FileDescriptorInfo = crate::ipc::codec::decode(&bytes).unwrap();
                assert_eq!((decoded.inode, decoded.peer_inode), (inode, peer_inode));
            }
        }
    }

    #[test]
    fn optional_peer_inode_accepts_missing_and_null_but_rejects_invalid_numbers() {
        let mut json = serde_json::to_value(descriptor_with_inodes(42, None)).unwrap();
        json.as_object_mut().unwrap().remove("peer_inode");
        assert!(
            serde_json::from_value::<FileDescriptorInfo>(json.clone())
                .unwrap()
                .peer_inode
                .is_none()
        );
        for invalid in ["-1", "18446744073709551616", "invalid"] {
            for field in ["inode", "peer_inode"] {
                let mut invalid_json = json.clone();
                invalid_json[field] = invalid.into();
                assert!(serde_json::from_value::<FileDescriptorInfo>(invalid_json).is_err());
            }
        }
    }
}
