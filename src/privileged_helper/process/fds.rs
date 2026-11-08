//! Acquire file descriptor metadata, communication peers and shared holders for one process.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`collect_file_descriptors`] | `pub(super)` | `fn collect_file_descriptors(id: `[`ProcessId`]`) -> `[`Result`]`<`[`FileDescriptors`]`>` |

use crate::ipc::model::socket_types::{
    AddressFamily, SocketConnectionStatus, SocketProtocol, SocketState, SocketType,
};
use crate::ipc::model::socket_types::{FdAccess, FdKind, InetAddress};
use anyhow::Result;
use std::{
    collections::{HashMap, HashSet},
    fs,
    net::SocketAddr,
    os::unix::fs::{FileTypeExt, MetadataExt},
    time::{Duration, Instant},
};
use {
    super::{permission_help, procfs, timestamp_ms, validate_process_identity},
    crate::{
        ipc::{model::sockets::SocketInfo, schema::ProcessId},
        privileged_helper::process::sockets,
    },
};

use crate::ipc::descriptors::{Endpoint, FileDescriptorInfo, FileDescriptors, Key, Owner};

fn parse_link_inode(link: &str, prefix: &str) -> Option<u64> {
    link.strip_prefix(prefix)?.strip_suffix(']')?.parse().ok()
}

fn identify_descriptor_resource(path: &std::path::Path, link: &str, fifo: bool) -> Option<Key> {
    if let Some(inode) = parse_link_inode(link, "socket:[") {
        return Some(Key {
            kind: FdKind::Socket,
            device: 0,
            inode,
        });
    }
    if let Some(inode) = parse_link_inode(link, "pipe:[") {
        return Some(Key {
            kind: FdKind::Pipe,
            device: 0,
            inode,
        });
    }
    if fifo {
        let meta = fs::metadata(path).ok()?;
        if meta.file_type().is_fifo() {
            return Some(Key {
                kind: FdKind::Fifo,
                device: meta.dev(),
                inode: meta.ino(),
            });
        }
    }
    None
}

fn read_descriptor_access_mode(pid: i32, fd: u32) -> Option<u32> {
    let fields = procfs::read_procfs_fields(&format!("/proc/{pid}/fdinfo/{fd}")).ok()?;
    // O_PATH descriptors cannot send or receive, even if O_ACCMODE is zero.
    let flags = u32::from_str_radix(fields.get("flags")?, 8).ok()?;
    if flags & libc::O_PATH as u32 != 0 {
        return None;
    }
    Some(flags & libc::O_ACCMODE as u32)
}

fn descriptor_access_from_mode(mode: Option<u32>) -> FdAccess {
    use crate::ipc::model::socket_types::fd_access_from_mode;

    fd_access_from_mode(mode)
}

fn collect_descriptor_owners(
    keys: &HashSet<Key>,
    deadline: Instant,
    warnings: &mut Vec<String>,
) -> Result<HashMap<Key, Vec<Owner>>> {
    let mut result: HashMap<Key, Vec<Owner>> = HashMap::new();
    let mut denied = 0;
    let mut count = 0;
    let mut matches = 0;
    let fifo = keys.iter().any(|k| k.kind == FdKind::Fifo);
    let mut pids: Vec<i32> = fs::read_dir("/proc")?
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse().ok())
        .collect();
    pids.sort_unstable();
    'processes: for pid in pids {
        if Instant::now() >= deadline {
            warnings.push(
                "The PID/FD scan reached its 3-second limit. The peer list is incomplete.".into(),
            );
            break;
        }
        let Ok(stat) = procfs::read_process_stat(&format!("/proc/{pid}/stat")) else {
            continue;
        };
        let id = ProcessId {
            pid,
            start_time_ticks: stat.start_time,
        };
        let directory = match fs::read_dir(format!("/proc/{pid}/fd")) {
            Ok(d) => d,
            Err(e) => {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    denied += 1;
                }
                continue;
            }
        };
        let mut found = Vec::new();
        for entry in directory.flatten() {
            count += 1;
            if count > 100_000 || matches >= 8192 || Instant::now() >= deadline {
                warnings.push(
                    "The PID/FD scan reached its count or time limit. The peer list is incomplete."
                        .into(),
                );
                break 'processes;
            }
            let Some(fd) = entry
                .file_name()
                .to_str()
                .and_then(|s| s.parse::<u32>().ok())
            else {
                continue;
            };
            let link = match fs::read_link(entry.path()) {
                Ok(p) => p.to_string_lossy().into_owned(),
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::PermissionDenied {
                        denied += 1;
                    }
                    continue;
                }
            };
            let Some(key) = identify_descriptor_resource(&entry.path(), &link, fifo)
                .filter(|k| keys.contains(k))
            else {
                continue;
            };
            let mode = read_descriptor_access_mode(pid, fd);
            found.push((
                key,
                Owner {
                    endpoint: Endpoint {
                        process_id: id,
                        name: stat.name.clone(),
                        fd,
                        access: descriptor_access_from_mode(mode),
                        relation: String::new(),
                    },
                    mode,
                },
            ));
            matches += 1;
        }
        if validate_process_identity(id).is_ok() {
            for (key, owner) in found {
                result.entry(key).or_default().push(owner);
            }
        }
    }
    if denied > 0 {
        warnings.push(format!(
            "Access was denied for {denied} processes/FDs. Some peers may be missing."
        ));
    }
    Ok(result)
}

fn socket_table(
    id: ProcessId,
    unix_needed: &HashSet<u64>,
    deadline: Instant,
    warnings: &mut Vec<String>,
) -> HashMap<u64, SocketInfo> {
    let pid = id.pid;
    let mut result = HashMap::new();
    result.extend(super::additional_sockets(pid));
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
        match sockets::read_socket_table(&format!("/proc/{pid}/net/{file}")) {
            Ok(text) => result.extend(if file == "unix" {
                sockets::parse_unix_socket_table(&text)
            } else {
                sockets::parse_inet_socket_table(&text, protocol)
            }),
            Err(e) => warnings.push(format!(
                "Could not read connection information from {file}: {e}"
            )),
        }
    }
    if unix_needed.iter().any(|inode| {
        result
            .get(inode)
            .is_some_and(|s| matches!(s.protocol, SocketProtocol::Unix { .. }))
    }) {
        let diag = super::open_network_namespace(id).and_then(|namespace| {
            sockets::collect_unix_socket_peers(
                &namespace,
                deadline.min(Instant::now() + Duration::from_millis(750)),
            )
        });
        match diag {
            Ok(diag) => result.extend(diag),
            Err(e) => warnings.push(format!(
                "Could not identify UNIX socket peers for PID {pid}: {e:#}"
            )),
        }
    }
    result
}

type InetKey = (bool, SocketAddr, SocketAddr);

fn connected_socket_key(info: &SocketInfo) -> Option<InetKey> {
    use crate::ipc::model::socket_types::{socket_protocol_is_tcp, socket_protocol_is_tcp_or_udp};

    if !socket_protocol_is_tcp_or_udp(info.protocol) {
        return None;
    }
    let local = info.local?;
    let remote = info.remote?;
    if info.state == Some(SocketState::Listen) || remote.port() == 0 || remote.ip().is_unspecified()
    {
        return None;
    }
    Some((socket_protocol_is_tcp(info.protocol), local, remote))
}

pub(super) fn collect_file_descriptors(id: ProcessId) -> Result<FileDescriptors> {
    use crate::{
        ipc::model::sockets::connection_status_socket_info,
        privileged_helper::process::{
            create_socket_metadata_reader, read_socket_metadata, socket_metadata_has_observation,
        },
    };

    validate_process_identity(id)?;
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut warnings = Vec::new();
    let mut entries = Vec::new();
    let directory = fs::read_dir(format!("/proc/{}/fd", id.pid))
        .map_err(|e| anyhow::anyhow!(permission_help("/proc/PID/fd", e)))?;
    let mut omitted = 0;
    for (index, entry) in directory.flatten().enumerate() {
        if index >= 16384 || entries.len() >= 4096 || Instant::now() >= deadline {
            warnings.push("The FD limit was reached. Showing partial results.".into());
            break;
        }
        let Some(fd) = entry
            .file_name()
            .to_str()
            .and_then(|s| s.parse::<u32>().ok())
        else {
            continue;
        };
        let Ok(link) = fs::read_link(entry.path()) else {
            omitted += 1;
            continue;
        };
        let link = link.to_string_lossy().into_owned();
        let Some(key) = identify_descriptor_resource(&entry.path(), &link, true) else {
            continue;
        };
        let mode = read_descriptor_access_mode(id.pid, fd);
        entries.push(FileDescriptorInfo {
            fd,
            kind: key.kind,
            inode: key.inode,
            target: link,
            access: descriptor_access_from_mode(mode),
            protocol: None,
            state: None,
            connection_status: (key.kind == FdKind::Socket)
                .then_some(SocketConnectionStatus::Unknown),
            local: None,
            remote: None,
            path: None,
            peer_inode: None,
            peers: Vec::new(),
            holders: Vec::new(),
            note: String::new(),
            key,
            mode,
            socket_device: if key.kind == FdKind::Socket {
                fs::metadata(entry.path()).ok().map(|m| m.dev())
            } else {
                None
            },
        });
    }
    if omitted > 0 {
        warnings.push(format!(
            "Could not read {omitted} FDs because they closed or access was denied."
        ));
    }
    entries.sort_by_key(|e| e.fd);
    let needed: HashSet<_> = entries
        .iter()
        .filter(|e| e.kind == FdKind::Socket)
        .map(|e| e.key.inode)
        .collect();
    let mut sockets = if needed.is_empty() {
        HashMap::new()
    } else {
        socket_table(id, &needed, deadline, &mut warnings)
    };
    let mut metadata_reader = create_socket_metadata_reader(id);
    let mut metadata = HashMap::new();
    for entry in &entries {
        if entry.kind != FdKind::Socket {
            continue;
        }
        if Instant::now() >= deadline {
            warnings
                .push("Socket metadata scan time limit reached. Showing partial results.".into());
            break;
        }
        if let Some(device) = entry.socket_device {
            let key = (device, entry.key.inode);
            if !metadata.contains_key(&key)
                && let Ok(observed) =
                    read_socket_metadata(&mut metadata_reader, entry.fd, device, entry.key.inode)
                && socket_metadata_has_observation(&observed)
            {
                metadata.insert(key, observed);
            }
        }
    }
    for (&(_, inode), observed) in &metadata {
        if let Some(protocol) = observed.protocol {
            sockets
                .entry(inode)
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
    let mut unavailable = 0;
    let mut unknown_statuses = 0;
    for entry in &mut entries {
        if entry.kind != FdKind::Socket {
            continue;
        }
        let observed = entry
            .socket_device
            .and_then(|device| metadata.get(&(device, entry.key.inode)))
            .map_or(SocketConnectionStatus::Unknown, |m| m.connection_status);
        let info = sockets.get(&entry.key.inode);
        let status = info.map_or(observed, |info| {
            connection_status_socket_info(info, observed)
        });
        entry.connection_status = Some(status);
        unavailable += usize::from(info.is_none());
        unknown_statuses += usize::from(status == SocketConnectionStatus::Unknown);
    }
    if unavailable > 0 {
        warnings.push(format!(
            "Socket type unavailable for {unavailable} FDs (closed, unsupported, or access denied)."
        ));
    }
    if unknown_statuses > 0 {
        warnings.push(format!("Socket connection status unavailable for {unknown_statuses} FDs (closed, unobserved, unsupported, or access denied)."));
    }
    let mut reversed: HashMap<InetKey, Vec<u64>> = HashMap::new();
    for (&inode, info) in &sockets {
        if let Some(key) = connected_socket_key(info) {
            reversed.entry(key).or_default().push(inode);
        }
    }
    let mut peer_keys: HashMap<u32, Vec<Key>> = HashMap::new();
    let mut keys: HashSet<Key> = entries.iter().map(|e| e.key).collect();
    for entry in &mut entries {
        if let Some(info) = sockets
            .get(&entry.key.inode)
            .filter(|_| entry.kind == FdKind::Socket)
        {
            entry.protocol = Some(info.protocol);
            entry.state = info.state;
            entry.local = info.local.map(InetAddress::from);
            entry.remote = info.remote.map(InetAddress::from);
            entry.path = info.path.clone();
            let mut peers = Vec::new();
            if let Some(inode) = info.peer_inode {
                entry.peer_inode = Some(inode);
                peers.push(inode);
            } else if let Some((tcp, local, remote)) = connected_socket_key(info)
                && let Some(inodes) = reversed.get(&(tcp, remote, local))
            {
                peers.extend(inodes.iter().copied().filter(|i| *i != entry.key.inode));
            }
            let peers: Vec<_> = peers
                .into_iter()
                .map(|inode| Key {
                    kind: FdKind::Socket,
                    device: 0,
                    inode,
                })
                .collect();
            keys.extend(peers.iter().copied());
            peer_keys.insert(entry.fd, peers);
        }
    }
    let owners = if keys.is_empty() {
        HashMap::new()
    } else {
        collect_descriptor_owners(&keys, deadline, &mut warnings)?
    };
    validate_process_identity(id)?;
    let result = FileDescriptors {
        process_id: id,
        captured_at: timestamp_ms(),
        entries,
        warnings,
        raw_fd_owners: owners.into_iter().collect(),
        raw_peer_keys: peer_keys.into_iter().collect(),
    };
    #[cfg(test)]
    let result = {
        let mut result = result;
        crate::http_server::analysis::descriptors::resolve_descriptor_relations(&mut result);
        result
    };
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reverse_matching_excludes_listeners_and_unconnected_sockets() {
        use crate::ipc::model::socket_types::socket_protocol_from_codes;

        let mut info = SocketInfo {
            protocol: SocketProtocol::Tcp {
                family: AddressFamily::Ipv4,
            },
            state: Some(SocketState::Listen),
            local: Some("127.0.0.1:80".parse().unwrap()),
            remote: Some("127.0.0.1:1234".parse().unwrap()),
            path: None,
            peer_inode: None,
        };
        assert!(connected_socket_key(&info).is_none());
        info.state = Some(SocketState::Established);
        let transport = info.protocol;
        info.protocol = socket_protocol_from_codes(
            libc::AF_INET as u32,
            libc::SOCK_RAW as u32,
            libc::IPPROTO_TCP as u32,
        );
        assert!(connected_socket_key(&info).is_none());
        info.protocol = transport;
        assert!(connected_socket_key(&info).is_some());
        info.remote = Some("0.0.0.0:0".parse().unwrap());
        assert!(connected_socket_key(&info).is_none());
    }
}
