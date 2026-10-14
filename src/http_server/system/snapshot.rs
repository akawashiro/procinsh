//! Collect process structure and descriptor relations, retaining kinds without peers.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`ProcessSnapshot`] | `pub(super)` | `struct ProcessSnapshot` |
//! | [`FdEndpoint`] | `pub(super)` | `struct FdEndpoint` |
//! | [`SocketEndpoint`] | `pub(super)` | `struct SocketEndpoint` |
//! | [`FdRelation`] | `pub(super)` | `struct FdRelation` |
//! | [`SystemSnapshot`] | `pub(in crate::http_server)` | `struct SystemSnapshot` |
//! | [`process_from_summary`] | `pub(super)` | `fn process_from_summary(summary: `[`ProcessSummary`]`, parent_id: `[`Option`]`<`[`ProcessId`]`>) -> `[`ProcessSnapshot`] |
//! | [`collect`] | `pub(super)` | `fn collect(discovery: &mut `[`ProcessScanner`]`) -> `[`SystemSnapshot`] |

use crate::http_server::process::{
    self, MemoryMap, ProcessId, ProcessScanner, ProcessSummary, SocketInfo,
};
use crate::http_server::resource::{DeviceId, IpcIdentity, IpcKind};
use crate::http_server::socket_types::{
    AddressFamily, SocketConnectionStatus, SocketProtocol, SocketState, SocketType,
};
use crate::http_server::socket_types::{FdAccess, FdKind, InetAddress};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    os::unix::fs::{FileTypeExt, MetadataExt},
    time::{Duration, Instant},
};
/// A Linux process and its observed structural address space, without RSS/PSS.
/// CPU and RSS measurements belong to [`crate::http_server::process::ProcessSummary`].
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct ProcessSnapshot {
    pub(super) identity: ProcessId,
    pub(super) parent_id: Option<ProcessId>,
    pub(super) name: String,
    /// Complete argument vector from /proc; absent when it cannot be read.
    pub(super) command_line: Option<Vec<String>>,
    pub(super) uid: Option<u32>,
    pub(super) username: Option<String>,
    pub(super) euid: Option<u32>,
    pub(super) effective_username: Option<String>,
    pub(super) maps: Vec<MemoryMap>,
    pub(super) maps_epoch: u64,
    pub(super) maps_error: Option<String>,
}
/// A process-owned file descriptor endpoint, including coalesced shared FDs.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FdEndpoint {
    pub(super) process_id: ProcessId,
    pub(super) fd: u32,
    pub(super) fd_count: usize,
    pub(super) resource: IpcIdentity,
    pub(super) kind: FdKind,
    pub(super) access: FdAccess,
    /// Independent connection observation; null for non-socket resources.
    pub(super) connection_status: Option<SocketConnectionStatus>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct SocketEndpoint {
    pub(super) protocol: SocketProtocol,
    /// Null when no state was observed, even if the socket kind is known.
    pub(super) state: Option<SocketState>,
    pub(super) local: Option<InetAddress>,
    pub(super) remote: Option<InetAddress>,
    pub(super) path: Option<String>,
    pub(super) network_peer: bool,
    pub(super) remote_hostname: Option<String>,
}
impl From<&SocketInfo> for SocketEndpoint {
    fn from(info: &SocketInfo) -> Self {
        Self {
            protocol: info.protocol,
            remote_hostname: None,
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
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct FdRelation {
    pub(super) id: String,
    pub(super) endpoint: FdEndpoint,
    pub(super) peer: Option<FdEndpoint>,
    pub(super) label: String,
    pub(super) socket: Option<SocketEndpoint>,
    pub(super) candidate: bool,
    pub(super) shared: bool,
}
/// A system-wide observation of processes and their file descriptor relations.
#[derive(Clone, Default, Serialize, Deserialize)]
// Re-exported by system for subscription consumers.
pub(in crate::http_server) struct SystemSnapshot {
    pub(super) captured_at: u64,
    pub(super) processes: Vec<ProcessSnapshot>,
    pub(super) fd_relations: Vec<FdRelation>,
    pub(super) warnings: Vec<String>,
    pub(super) inspected_processes: usize,
    pub(super) inspected_fds: usize,
}

pub(super) fn process_from_summary(
    summary: ProcessSummary,
    parent_id: Option<ProcessId>,
) -> ProcessSnapshot {
    ProcessSnapshot {
        identity: summary.identity,
        parent_id,
        name: summary.name,
        command_line: summary.command_line,
        uid: summary.uid,
        username: summary.username,
        euid: summary.euid,
        effective_username: summary.effective_username,
        maps: vec![],
        maps_epoch: 0,
        maps_error: None,
    }
}

pub(super) fn collect(discovery: &mut ProcessScanner) -> SystemSnapshot {
    let mut result = SystemSnapshot {
        captured_at: process::timestamp_ms(),
        ..Default::default()
    };
    let summaries = match discovery.collect() {
        Ok(v) => v,
        Err(e) => {
            result.warnings.push(e.to_string());
            return result;
        }
    };
    let identities: HashMap<_, _> = summaries
        .iter()
        .map(|summary| (summary.identity.pid, summary.identity))
        .collect();
    let mut owners: HashMap<IpcIdentity, Vec<FdEndpoint>> = HashMap::new();
    let mut infos = HashMap::new();
    let mut inode_ns = HashMap::new();
    let mut namespaces = HashSet::new();
    let mut namespace_readers = Vec::new();
    let mut denied = 0;
    let mut metadata: HashMap<IpcIdentity, process::SocketMetadata> = HashMap::new();
    let mut truncated = false;
    let deadline = Instant::now() + Duration::from_secs(8);
    for summary in summaries {
        let parent_id = identities
            .get(&summary.parent_pid)
            .copied()
            .filter(|parent| *parent != summary.identity);
        let mut n = process_from_summary(summary, parent_id);
        let pid = n.identity.pid;
        let mut metadata_reader = process::SocketMetadataReader::new(n.identity);
        if Instant::now() >= deadline {
            n.maps_error = Some("Scan time limit reached. Retrying on the next update.".into());
            truncated = true;
            result.processes.push(n);
            continue;
        }
        n.maps_epoch = super::monotonic_ns();
        match process::memory_maps(pid) {
            Ok(mut m) => {
                if m.len() > 4096 {
                    m.truncate(4096);
                    truncated = true;
                }
                n.maps = m;
            }
            Err(e) => n.maps_error = Some(e.to_string()),
        }
        if let Ok(ns) = fs::metadata(format!("/proc/{pid}/ns/net"))
            && namespaces.insert((ns.dev(), ns.ino()))
        {
            match process::NetworkNamespaceReader::open(n.identity) {
                Ok(reader) => namespace_readers.push((n.identity, reader)),
                Err(e) => result
                    .warnings
                    .push(format!("UNIX peer namespace for PID {pid}: {e:#}")),
            }
            for (file, protocol) in [
                (
                    "tcp",
                    SocketProtocol::Tcp {
                        family: AddressFamily::Ipv4,
                    },
                ),
                (
                    "tcp6",
                    SocketProtocol::Tcp {
                        family: AddressFamily::Ipv6,
                    },
                ),
                (
                    "udp",
                    SocketProtocol::Udp {
                        family: AddressFamily::Ipv4,
                    },
                ),
                (
                    "udp6",
                    SocketProtocol::Udp {
                        family: AddressFamily::Ipv6,
                    },
                ),
                (
                    "unix",
                    SocketProtocol::Unix {
                        socket_type: SocketType::Unknown(0),
                    },
                ),
            ] {
                if let Ok(text) = process::socket_text(&format!("/proc/{pid}/net/{file}")) {
                    let table = if file == "unix" {
                        process::unix_sockets(&text)
                    } else {
                        process::inet_sockets(&text, protocol)
                    };
                    for inode in table.keys() {
                        inode_ns.insert(*inode, (ns.dev(), ns.ino()));
                    }
                    infos.extend(table);
                }
            }
            infos.extend(process::additional_sockets(pid));
        }
        let mut endpoints = Vec::new();
        match fs::read_dir(format!("/proc/{pid}/fd")) {
            Ok(fds) => {
                result.inspected_processes += 1;
                for fd in fds.flatten() {
                    if result.inspected_fds >= 100_000 || Instant::now() >= deadline {
                        truncated = true;
                        break;
                    }
                    result.inspected_fds += 1;
                    let Ok(number) = fd.file_name().to_string_lossy().parse::<u32>() else {
                        continue;
                    };
                    let meta = match fs::metadata(fd.path()) {
                        Ok(m) => m,
                        Err(e) => {
                            if e.kind() == std::io::ErrorKind::PermissionDenied {
                                denied += 1;
                            }
                            continue;
                        }
                    };
                    let kind = if meta.file_type().is_socket() {
                        FdKind::Socket
                    } else if meta.file_type().is_fifo() {
                        FdKind::Pipe
                    } else {
                        continue;
                    };
                    let resource = IpcIdentity {
                        kind: if kind == FdKind::Pipe {
                            IpcKind::Pipe
                        } else {
                            IpcKind::Socket
                        },
                        device: DeviceId::from_stat(meta.dev()),
                        inode: meta.ino(),
                    };
                    if kind == FdKind::Socket
                        && !metadata.contains_key(&resource)
                        && let Ok(observed) = metadata_reader.read(number, meta.dev(), meta.ino())
                        && observed.has_observation()
                    {
                        metadata.insert(resource, observed);
                    }
                    let flags = process::fields(&format!("/proc/{pid}/fdinfo/{number}"))
                        .ok()
                        .and_then(|f| f.get("flags").and_then(|v| u32::from_str_radix(v, 8).ok()))
                        .unwrap_or(u32::MAX);
                    endpoints.push(FdEndpoint {
                        process_id: n.identity,
                        fd: number,
                        fd_count: 1,
                        resource,
                        connection_status: (kind == FdKind::Socket)
                            .then_some(SocketConnectionStatus::Unknown),
                        kind,
                        access: FdAccess::from_flags(flags),
                    });
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    denied += 1;
                }
            }
        }
        if process::check_identity(n.identity).is_ok() {
            for p in endpoints {
                let list = owners.entry(p.resource).or_default();
                if let Some(existing) = list
                    .iter_mut()
                    .find(|e| e.process_id == p.process_id && e.access == p.access)
                {
                    existing.fd = existing.fd.min(p.fd);
                    existing.fd_count += 1;
                } else {
                    list.push(p);
                }
            }
            result.processes.push(n);
        }
    }
    let diag_deadline = Instant::now() + Duration::from_millis(500);
    for (id, namespace) in namespace_readers {
        if Instant::now() >= diag_deadline {
            result.warnings.push(
                "UNIX peer namespace scan reached its 500ms limit. Some peers may be missing."
                    .into(),
            );
            break;
        }
        match process::unix_socket_peers(&namespace, diag_deadline) {
            Ok(diag) => infos.extend(diag),
            Err(e) => result.warnings.push(format!(
                "UNIX peer for PID {} / namespace {:?}: {e:#}",
                id.pid,
                namespace.key()
            )),
        }
    }
    // Enrich after all table/diag reads so direct observations cannot be overwritten.
    for (resource, observed) in &metadata {
        if let Some(protocol) = observed.protocol {
            infos
                .entry(resource.inode)
                .and_modify(|info| {
                    if matches!(
                        info.protocol,
                        SocketProtocol::Other {
                            socket_type: None,
                            ..
                        }
                    ) {
                        info.protocol = protocol;
                    }
                })
                .or_insert(SocketInfo {
                    protocol,
                    state: None,
                    local: None,
                    remote: None,
                    path: None,
                    peer_inode: None,
                });
        }
    }
    let mut unknown_socket_types = 0;
    let mut unknown_connection_statuses = 0;
    for (resource, endpoints) in &mut owners {
        if resource.kind != IpcKind::Socket {
            continue;
        }
        let observed = metadata
            .get(resource)
            .map_or(SocketConnectionStatus::Unknown, |m| m.connection_status);
        let info = infos.get(&resource.inode);
        let status = info.map_or(observed, |info| info.connection_status(observed));
        for endpoint in endpoints {
            endpoint.connection_status = Some(status);
            if info.is_none() {
                unknown_socket_types += endpoint.fd_count;
            }
            if status == SocketConnectionStatus::Unknown {
                unknown_connection_statuses += endpoint.fd_count;
            }
        }
    }
    // Socket inodes are global, but reversed tuples must also share a namespace. Build candidates from same net table below only if unique.
    let mut reverse: HashMap<_, Vec<u64>> = HashMap::new();
    for (&inode, info) in &infos {
        if info.protocol.is_tcp_or_udp()
            && let (Some(local), Some(remote)) = (info.local, info.remote)
            && remote.port() != 0
            && !remote.ip().is_unspecified()
            && info.state != Some(SocketState::Listen)
        {
            reverse
                .entry((
                    inode_ns.get(&inode).copied(),
                    info.protocol.is_tcp(),
                    local,
                    remote,
                ))
                .or_default()
                .push(inode);
        }
    }
    let mut by_inode = HashMap::new();
    for key in owners.keys() {
        if key.kind == IpcKind::Socket {
            by_inode.insert(key.inode, *key);
        }
    }
    for endpoints in owners.values_mut() {
        endpoints.sort_by_key(|p| (p.process_id.pid, p.process_id.start_time_ticks, p.fd));
    }
    let mut seen = HashSet::new();
    for (key, endpoints) in &owners {
        for endpoint in endpoints {
            let mut matches: Vec<(&FdEndpoint, bool, bool, String)> = Vec::new();
            for peer in endpoints {
                if endpoint.process_id == peer.process_id && endpoint.fd == peer.fd {
                    continue;
                }
                let shared =
                    endpoint.kind == FdKind::Socket || !endpoint.access.opposite(peer.access);
                // Shared ownership is a group, not a full mesh of communication links.
                if shared
                    && !std::ptr::eq(endpoint, &endpoints[0])
                    && !std::ptr::eq(peer, &endpoints[0])
                {
                    continue;
                }
                matches.push((peer, false, shared, endpoint.kind.to_string()));
            }
            let inode = key.inode;
            let info = if endpoint.kind == FdKind::Socket {
                infos.get(&inode)
            } else {
                None
            };
            if let Some(info) = info {
                let mut peers = Vec::new();
                if let Some(peer) = info.peer_inode {
                    peers.push((peer, false));
                } else if info.protocol.is_tcp_or_udp()
                    && let (Some(local), Some(remote)) = (info.local, info.remote)
                    && let Some(ids) = reverse.get(&(
                        inode_ns.get(&inode).copied(),
                        info.protocol.is_tcp(),
                        remote,
                        local,
                    ))
                {
                    peers.extend(ids.iter().filter(|&&v| v != inode).map(|&v| (v, true)));
                }
                for (peer, candidate) in peers {
                    if let Some(peer_endpoints) = by_inode.get(&peer).and_then(|k| owners.get(k)) {
                        for peer in peer_endpoints {
                            matches.push((peer, candidate, false, info.protocol.to_string()));
                        }
                    }
                }
            }
            // Shared ownership does not identify a network receiver.
            if matches.is_empty()
                || (info.is_some_and(|info| SocketEndpoint::from(info).network_peer)
                    && matches.iter().all(|(_, _, shared, _)| *shared))
            {
                let label = info
                    .map(|i| {
                        format!(
                            "{} {} {}",
                            i.protocol,
                            i.state.map(|s| s.to_string()).unwrap_or_default(),
                            i.remote
                                .map(|v| v.to_string())
                                .or(i.path.clone())
                                .unwrap_or_default()
                        )
                    })
                    .map(|label| label.trim().to_owned())
                    .unwrap_or_else(|| {
                        if endpoint.kind == FdKind::Socket {
                            "Socket · Type unavailable".into()
                        } else {
                            "Unknown peer".into()
                        }
                    });
                result.fd_relations.push(FdRelation {
                    id: format!(
                        "{}:{}:{}:external",
                        endpoint.process_id.pid, endpoint.process_id.start_time_ticks, endpoint.fd
                    ),
                    endpoint: endpoint.clone(),
                    peer: None,
                    label,
                    socket: info.map(SocketEndpoint::from),
                    candidate: false,
                    shared: false,
                });
            }
            matches.sort_by_key(|(_, _, shared, _)| *shared);
            if matches.len() > 64 {
                truncated = true;
            }
            for (peer, candidate, shared, label) in matches.into_iter().take(64) {
                let label = if endpoint.kind == FdKind::Socket {
                    info.map(|i| i.protocol.to_string())
                        .unwrap_or_else(|| "Socket · Type unavailable".into())
                } else {
                    label
                };
                let mut ends = [
                    format!(
                        "{}:{}:{}",
                        endpoint.process_id.pid, endpoint.process_id.start_time_ticks, endpoint.fd
                    ),
                    format!(
                        "{}:{}:{}",
                        peer.process_id.pid, peer.process_id.start_time_ticks, peer.fd
                    ),
                ];
                ends.sort();
                let id = ends.join("/");
                if seen.insert(id.clone()) {
                    result.fd_relations.push(FdRelation {
                        id,
                        endpoint: endpoint.clone(),
                        peer: Some(peer.clone()),
                        label,
                        socket: info.map(SocketEndpoint::from),
                        candidate,
                        shared,
                    });
                }
            }
            if result.fd_relations.len() >= 20_000 {
                truncated = true;
                break;
            }
        }
        if result.fd_relations.len() >= 20_000 {
            break;
        }
    }
    if denied > 0 {
        result
            .warnings
            .push(format!("Access denied for {denied} processes/FDs"));
    }
    if unknown_socket_types > 0 {
        result.warnings.push(format!(
            "Socket type unavailable for {unknown_socket_types} FDs (closed, unobserved, unsupported, or access denied)."
        ));
    }
    if unknown_connection_statuses > 0 {
        result.warnings.push(format!(
            "Socket connection status unavailable for {unknown_connection_statuses} FDs (closed, unobserved, unsupported, or access denied)."
        ));
    }
    if truncated {
        result
            .warnings
            .push("Scan, mapping, or connection limit reached. Some process or FD information is missing.".into());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn socket_destination_classification() {
        use crate::http_server::process::SocketInfo;
        let mut info = SocketInfo {
            protocol: SocketProtocol::Tcp {
                family: AddressFamily::Ipv6,
            },
            state: Some(SocketState::Established),
            local: Some("[::1]:5000".parse().unwrap()),
            remote: Some("[2001:db8::1]:443".parse().unwrap()),
            path: None,
            peer_inode: None,
        };
        let endpoint = SocketEndpoint::from(&info);
        assert!(endpoint.network_peer);
        assert_eq!(
            serde_json::to_value(endpoint).unwrap()["remote"],
            serde_json::json!({"ip":"2001:db8::1","port":443})
        );
        info.state = Some(SocketState::Listen);
        assert!(!SocketEndpoint::from(&info).network_peer);
        info.protocol = SocketProtocol::Udp {
            family: AddressFamily::Ipv4,
        };
        info.state = Some(SocketState::Unconnected);
        for address in ["0.0.0.0:0", "127.0.0.1:0", "[::]:123"] {
            info.remote = Some(address.parse().unwrap());
            assert!(!SocketEndpoint::from(&info).network_peer);
        }
        info.remote = None;
        assert!(!SocketEndpoint::from(&info).network_peer);
        info.protocol = SocketProtocol::Unix {
            socket_type: SocketType::Unknown(0),
        };
        assert!(!SocketEndpoint::from(&info).network_peer);
        info.protocol = SocketProtocol::from_codes(
            libc::AF_INET as u32,
            libc::SOCK_RAW as u32,
            libc::IPPROTO_TCP as u32,
        );
        info.remote = Some("127.0.0.1:443".parse().unwrap());
        assert!(!SocketEndpoint::from(&info).network_peer);
    }
}

#[cfg(test)]
#[path = "snapshot_tests.rs"]
mod fixture_tests;
