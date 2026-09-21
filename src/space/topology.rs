use crate::process::{
    self, ProcessId,
    discovery::{Discovery, ProcessSummary},
    maps::{self, MemoryMap},
    sockets,
};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    fs,
    os::unix::fs::{FileTypeExt, MetadataExt},
    time::{Duration, Instant},
};
#[derive(Clone, Serialize)]
pub struct Node {
    pub identity: ProcessId,
    pub parent_id: Option<ProcessId>,
    pub name: String,
    pub uid: Option<u32>,
    pub username: Option<String>,
    pub euid: Option<u32>,
    pub effective_username: Option<String>,
    pub cpu_percent: Option<f64>,
    pub rss_bytes: u64,
    pub maps: Vec<MemoryMap>,
    pub maps_epoch: u64,
    pub maps_error: Option<String>,
}
#[derive(Clone, Serialize)]
pub struct Port {
    pub process_id: ProcessId,
    pub fd: u32,
    pub fd_count: usize,
    pub resource: String,
    pub kind: String,
    pub access: u32,
}
#[derive(Clone, Serialize)]
pub struct SocketEndpoint {
    pub protocol: String,
    pub state: String,
    pub local: Option<std::net::SocketAddr>,
    pub remote: Option<std::net::SocketAddr>,
    pub network_peer: bool,
    pub remote_hostname: Option<String>,
}
impl From<&sockets::SocketInfo> for SocketEndpoint {
    fn from(info: &sockets::SocketInfo) -> Self {
        Self {
            protocol: info.protocol.clone(),
            remote_hostname: None,
            state: info.state.clone(),
            local: info.local,
            remote: info.remote,
            network_peer: (info.protocol.starts_with("TCP") || info.protocol.starts_with("UDP"))
                && info.state != "LISTEN"
                && info
                    .remote
                    .is_some_and(|remote| remote.port() != 0 && !remote.ip().is_unspecified()),
        }
    }
}
#[derive(Clone, Serialize)]
pub struct Edge {
    pub id: String,
    pub a: Port,
    pub b: Option<Port>,
    pub label: String,
    pub socket: Option<SocketEndpoint>,
    pub candidate: bool,
    pub shared: bool,
}
#[derive(Clone, Default, Serialize)]
pub struct Topology {
    pub captured_at: u64,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub warnings: Vec<String>,
    pub inspected_processes: usize,
    pub inspected_fds: usize,
}
pub fn node(summary: ProcessSummary, parent_id: Option<ProcessId>) -> Node {
    Node {
        identity: summary.identity,
        parent_id,
        name: summary.name,
        uid: summary.uid,
        username: summary.username,
        euid: summary.euid,
        effective_username: summary.effective_username,
        cpu_percent: summary.cpu_percent,
        rss_bytes: summary.rss_bytes,
        maps: vec![],
        maps_epoch: 0,
        maps_error: None,
    }
}
pub fn resource(kind: &str, dev: u64, inode: u64) -> String {
    format!("{kind}:{dev}:{inode}")
}
pub fn collect(discovery: &mut Discovery) -> Topology {
    let mut result = Topology {
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
    let mut owners: HashMap<String, Vec<Port>> = HashMap::new();
    let mut infos = HashMap::new();
    let mut inode_ns = HashMap::new();
    let mut namespaces = HashSet::new();
    let mut denied = 0;
    let mut truncated = false;
    let deadline = Instant::now() + Duration::from_secs(4);
    for summary in summaries {
        let parent_id = identities
            .get(&summary.parent_pid)
            .copied()
            .filter(|parent| *parent != summary.identity);
        let mut n = node(summary, parent_id);
        let pid = n.identity.pid;
        if Instant::now() >= deadline {
            n.maps_error = Some("Scan time limit reached. Retrying on the next update.".into());
            truncated = true;
            result.nodes.push(n);
            continue;
        }
        n.maps_epoch = super::monotonic_ns();
        match maps::read(pid, false) {
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
            && namespaces.insert(ns.ino())
        {
            for (file, protocol) in [
                ("tcp", "TCP"),
                ("tcp6", "TCP6"),
                ("udp", "UDP"),
                ("udp6", "UDP6"),
                ("unix", "UNIX"),
            ] {
                if let Ok(text) = sockets::read_text(&format!("/proc/{pid}/net/{file}")) {
                    let table = if file == "unix" {
                        sockets::parse_unix(&text)
                    } else {
                        sockets::parse_inet(&text, protocol)
                    };
                    for inode in table.keys() {
                        inode_ns.insert(*inode, ns.ino());
                    }
                    infos.extend(table);
                }
            }
        }
        let mut ports = Vec::new();
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
                        "socket"
                    } else if meta.file_type().is_fifo() {
                        "pipe"
                    } else {
                        continue;
                    };
                    let flags = process::procfs::fields(&format!("/proc/{pid}/fdinfo/{number}"))
                        .ok()
                        .and_then(|f| f.get("flags").and_then(|v| u32::from_str_radix(v, 8).ok()))
                        .unwrap_or(u32::MAX);
                    ports.push(Port {
                        process_id: n.identity,
                        fd: number,
                        fd_count: 1,
                        resource: resource(kind, meta.dev(), meta.ino()),
                        kind: kind.into(),
                        access: if flags & libc::O_PATH as u32 != 0 {
                            u32::MAX
                        } else {
                            flags & 3
                        },
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
            for p in ports {
                let list = owners.entry(p.resource.clone()).or_default();
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
            result.nodes.push(n);
        }
    }
    match sockets::unix_diag(Instant::now() + Duration::from_millis(500)) {
        Ok(diag) => infos.extend(diag),
        Err(e) => result.warnings.push(format!("UNIX peer: {e}")),
    }
    // Socket inodes are global, but reversed tuples must also share a namespace. Build candidates from same net table below only if unique.
    let mut reverse: HashMap<_, Vec<u64>> = HashMap::new();
    for (&inode, info) in &infos {
        if let (Some(local), Some(remote)) = (info.local, info.remote)
            && remote.port() != 0
            && !remote.ip().is_unspecified()
            && info.state != "LISTEN"
        {
            reverse
                .entry((
                    inode_ns.get(&inode).copied(),
                    info.protocol.starts_with("TCP"),
                    local,
                    remote,
                ))
                .or_default()
                .push(inode);
        }
    }
    let mut by_inode = HashMap::new();
    for key in owners.keys() {
        if key.starts_with("socket:")
            && let Some(inode) = key.rsplit(':').next().and_then(|v| v.parse::<u64>().ok())
        {
            by_inode.insert(inode, key.clone());
        }
    }
    for ports in owners.values_mut() {
        ports.sort_by_key(|p| (p.process_id.pid, p.process_id.start_time_ticks, p.fd));
    }
    let mut seen = HashSet::new();
    for (key, ports) in &owners {
        for a in ports {
            let mut matches: Vec<(&Port, bool, bool, String)> = Vec::new();
            for b in ports {
                if a.process_id == b.process_id && a.fd == b.fd {
                    continue;
                }
                let shared = a.kind == "socket"
                    || !matches!((a.access, b.access), (0, 1 | 2) | (1, 0 | 2) | (2, 0..=2));
                // Shared ownership is a group, not a full mesh of communication links.
                if shared && !std::ptr::eq(a, &ports[0]) && !std::ptr::eq(b, &ports[0]) {
                    continue;
                }
                matches.push((b, false, shared, a.kind.clone()));
            }
            let inode = key
                .rsplit(':')
                .next()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            let info = if a.kind == "socket" {
                infos.get(&inode)
            } else {
                None
            };
            if let Some(info) = info {
                let mut peers = Vec::new();
                if let Some(peer) = info.peer_inode {
                    peers.push((peer, false));
                } else if let (Some(local), Some(remote)) = (info.local, info.remote)
                    && let Some(ids) = reverse.get(&(
                        inode_ns.get(&inode).copied(),
                        info.protocol.starts_with("TCP"),
                        remote,
                        local,
                    ))
                {
                    peers.extend(ids.iter().filter(|&&v| v != inode).map(|&v| (v, true)));
                }
                for (peer, candidate) in peers {
                    if let Some(peerports) = by_inode.get(&peer).and_then(|k| owners.get(k)) {
                        for b in peerports {
                            matches.push((b, candidate, false, info.protocol.clone()));
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
                            i.state,
                            i.remote
                                .map(|v| v.to_string())
                                .or(i.path.clone())
                                .unwrap_or_default()
                        )
                    })
                    .unwrap_or_else(|| "Unknown peer".into());
                result.edges.push(Edge {
                    id: format!(
                        "{}:{}:{}:external",
                        a.process_id.pid, a.process_id.start_time_ticks, a.fd
                    ),
                    a: a.clone(),
                    b: None,
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
            for (b, candidate, shared, label) in matches.into_iter().take(64) {
                let mut ends = [
                    format!(
                        "{}:{}:{}",
                        a.process_id.pid, a.process_id.start_time_ticks, a.fd
                    ),
                    format!(
                        "{}:{}:{}",
                        b.process_id.pid, b.process_id.start_time_ticks, b.fd
                    ),
                ];
                ends.sort();
                let id = ends.join("/");
                if seen.insert(id.clone()) {
                    result.edges.push(Edge {
                        id,
                        a: a.clone(),
                        b: Some(b.clone()),
                        label,
                        socket: info.map(SocketEndpoint::from),
                        candidate,
                        shared,
                    });
                }
            }
            if result.edges.len() >= 20_000 {
                truncated = true;
                break;
            }
        }
        if result.edges.len() >= 20_000 {
            break;
        }
    }
    if denied > 0 {
        result
            .warnings
            .push(format!("Access denied for {denied} processes/FDs"));
    }
    if truncated {
        result
            .warnings
            .push("Scan, mapping, or connection limit reached. Some topology is missing.".into());
    }
    if namespaces.len() > 1 {
        result.warnings.push(
            "UNIX peers in other network namespaces are unobserved. TCP/UDP peers are address-based candidates.".into(),
        );
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn socket_destination_classification() {
        use crate::process::sockets::SocketInfo;
        let mut info = SocketInfo {
            protocol: "TCP6".into(),
            state: "ESTABLISHED".into(),
            local: Some("[::1]:5000".parse().unwrap()),
            remote: Some("[2001:db8::1]:443".parse().unwrap()),
            path: None,
            peer_inode: None,
        };
        let endpoint = SocketEndpoint::from(&info);
        assert!(endpoint.network_peer);
        assert_eq!(
            serde_json::to_value(endpoint).unwrap()["remote"],
            "[2001:db8::1]:443"
        );
        info.state = "LISTEN".into();
        assert!(!SocketEndpoint::from(&info).network_peer);
        info.protocol = "UDP".into();
        info.state = "UNCONN".into();
        for address in ["0.0.0.0:0", "127.0.0.1:0", "[::]:123"] {
            info.remote = Some(address.parse().unwrap());
            assert!(!SocketEndpoint::from(&info).network_peer);
        }
        info.remote = None;
        assert!(!SocketEndpoint::from(&info).network_peer);
        info.protocol = "UNIX".into();
        assert!(!SocketEndpoint::from(&info).network_peer);
    }
}
