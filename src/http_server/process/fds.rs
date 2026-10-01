use super::{
    ProcessId, check_identity, permission_help, procfs,
    sockets::{self, SocketInfo},
    timestamp_ms,
};
use crate::http_server::socket_types::{AddressFamily, SocketProtocol, SocketState, SocketType};
use crate::http_server::socket_types::{FdAccess, FdKind, InetAddress};
use anyhow::Result;
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    fs,
    net::SocketAddr,
    os::unix::fs::{FileTypeExt, MetadataExt},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct Key {
    kind: FdKind,
    device: u64,
    inode: u64,
}

#[derive(Clone, Debug, Serialize)]
pub(super) struct Endpoint {
    pub(super) process_id: ProcessId,
    pub(super) name: String,
    pub(super) fd: u32,
    pub(super) access: FdAccess,
    pub(super) relation: String,
}
#[derive(Debug, Serialize)]
pub(super) struct Descriptor {
    pub(super) fd: u32,
    pub(super) kind: FdKind,
    pub(super) inode: String,
    pub(super) target: String,
    pub(super) access: FdAccess,
    pub(super) protocol: Option<SocketProtocol>,
    pub(super) state: Option<SocketState>,
    pub(super) local: Option<InetAddress>,
    pub(super) remote: Option<InetAddress>,
    pub(super) path: Option<String>,
    pub(super) peer_inode: Option<String>,
    pub(super) peers: Vec<Endpoint>,
    pub(super) holders: Vec<Endpoint>,
    pub(super) note: String,
    #[serde(skip)]
    key: Key,
    #[serde(skip)]
    mode: Option<u32>,
}
#[derive(Debug, Serialize)]
pub(super) struct FileDescriptors {
    pub(super) process_id: ProcessId,
    pub(super) captured_at: u64,
    pub(super) entries: Vec<Descriptor>,
    pub(super) warnings: Vec<String>,
}

fn inode(link: &str, prefix: &str) -> Option<u64> {
    link.strip_prefix(prefix)?.strip_suffix(']')?.parse().ok()
}

fn resource(path: &std::path::Path, link: &str, fifo: bool) -> Option<Key> {
    if let Some(inode) = inode(link, "socket:[") {
        return Some(Key {
            kind: FdKind::Socket,
            device: 0,
            inode,
        });
    }
    if let Some(inode) = inode(link, "pipe:[") {
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

fn mode(pid: i32, fd: u32) -> Option<u32> {
    let fields = procfs::fields(&format!("/proc/{pid}/fdinfo/{fd}")).ok()?;
    // O_PATH descriptors cannot send or receive, even if O_ACCMODE is zero.
    let flags = u32::from_str_radix(fields.get("flags")?, 8).ok()?;
    if flags & libc::O_PATH as u32 != 0 {
        return None;
    }
    Some(flags & libc::O_ACCMODE as u32)
}

fn access(mode: Option<u32>) -> FdAccess {
    FdAccess::from_mode(mode)
}

fn pipe_opposite(a: Option<u32>, b: Option<u32>) -> bool {
    matches!(
        (a, b),
        (Some(0), Some(1 | 2)) | (Some(1), Some(0 | 2)) | (Some(2), Some(0..=2))
    )
}

struct Owner {
    endpoint: Endpoint,
    mode: Option<u32>,
}

fn owners(
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
        let Ok(stat) = procfs::read_stat(&format!("/proc/{pid}/stat")) else {
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
            let Some(key) = resource(&entry.path(), &link, fifo).filter(|k| keys.contains(k))
            else {
                continue;
            };
            let mode = mode(pid, fd);
            found.push((
                key,
                Owner {
                    endpoint: Endpoint {
                        process_id: id,
                        name: stat.name.clone(),
                        fd,
                        access: access(mode),
                        relation: String::new(),
                    },
                    mode,
                },
            ));
            matches += 1;
        }
        if check_identity(id).is_ok() {
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
    pid: i32,
    unix_needed: &HashSet<u64>,
    deadline: Instant,
    warnings: &mut Vec<String>,
) -> HashMap<u64, SocketInfo> {
    let mut result = HashMap::new();
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
        match sockets::read_text(&format!("/proc/{pid}/net/{file}")) {
            Ok(text) => result.extend(if file == "unix" {
                sockets::parse_unix(&text)
            } else {
                sockets::parse_inet(&text, protocol)
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
        let same_ns = fs::metadata(format!("/proc/{pid}/ns/net"))
            .ok()
            .zip(fs::metadata("/proc/self/ns/net").ok())
            .is_some_and(|(a, b)| a.ino() == b.ino() && a.dev() == b.dev());
        if same_ns {
            match sockets::unix_diag(deadline.min(Instant::now() + Duration::from_millis(750))) {
                Ok(diag) => result.extend(diag),
                Err(e) => warnings.push(format!("Could not identify UNIX socket peers: {e}")),
            }
        } else {
            warnings
                .push("UNIX socket peer inodes are unavailable across network namespaces.".into());
        }
    }
    result
}

type InetKey = (bool, SocketAddr, SocketAddr);

fn inet_key(info: &SocketInfo) -> Option<InetKey> {
    let local = info.local?;
    let remote = info.remote?;
    if info.state == SocketState::Listen || remote.port() == 0 || remote.ip().is_unspecified() {
        return None;
    }
    Some((info.protocol.is_tcp(), local, remote))
}

pub(super) fn read(id: ProcessId) -> Result<FileDescriptors> {
    check_identity(id)?;
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
        let Some(key) = resource(&entry.path(), &link, true) else {
            continue;
        };
        let mode = mode(id.pid, fd);
        entries.push(Descriptor {
            fd,
            kind: key.kind,
            inode: key.inode.to_string(),
            target: link,
            access: access(mode),
            protocol: None,
            state: None,
            local: None,
            remote: None,
            path: None,
            peer_inode: None,
            peers: Vec::new(),
            holders: Vec::new(),
            note: String::new(),
            key,
            mode,
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
    let sockets = if needed.is_empty() {
        HashMap::new()
    } else {
        socket_table(id.pid, &needed, deadline, &mut warnings)
    };
    let mut reversed: HashMap<InetKey, Vec<u64>> = HashMap::new();
    for (&inode, info) in &sockets {
        if let Some(key) = inet_key(info) {
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
            entry.state = Some(info.state);
            entry.local = info.local.map(InetAddress::from);
            entry.remote = info.remote.map(InetAddress::from);
            entry.path = info.path.clone();
            let mut peers = Vec::new();
            if let Some(inode) = info.peer_inode {
                entry.peer_inode = Some(inode.to_string());
                peers.push(inode);
            } else if let Some((tcp, local, remote)) = inet_key(info)
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
        owners(&keys, deadline, &mut warnings)?
    };
    for entry in &mut entries {
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
    check_identity(id)?;
    Ok(FileDescriptors {
        process_id: id,
        captured_at: timestamp_ms(),
        entries,
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_directions_are_not_confused_with_shared_ends() {
        assert!(pipe_opposite(Some(0), Some(1)));
        assert!(pipe_opposite(Some(1), Some(0)));
        assert!(pipe_opposite(Some(2), Some(0)));
        assert!(!pipe_opposite(Some(0), Some(0)));
        assert!(!pipe_opposite(Some(1), Some(1)));
        assert!(!pipe_opposite(None, Some(1)));
    }

    #[test]
    fn reverse_matching_excludes_listeners_and_unconnected_sockets() {
        let mut info = SocketInfo {
            protocol: SocketProtocol::Tcp {
                family: AddressFamily::Ipv4,
            },
            state: SocketState::Listen,
            local: Some("127.0.0.1:80".parse().unwrap()),
            remote: Some("127.0.0.1:1234".parse().unwrap()),
            path: None,
            peer_inode: None,
        };
        assert!(inet_key(&info).is_none());
        info.state = SocketState::Established;
        assert!(inet_key(&info).is_some());
        info.remote = Some("0.0.0.0:0".parse().unwrap());
        assert!(inet_key(&info).is_none());
    }
}
