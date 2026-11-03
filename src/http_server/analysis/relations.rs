//! Build FD relationships from captured endpoints and socket observations.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`build`] | `pub` | `fn build(result: &mut SystemSnapshot) -> bool` |
//!
//! Input and output values are defined by [`SystemSnapshot`].

use crate::http_server::model::{
    resource::IpcKind,
    socket_types::{FdKind, SocketConnectionStatus, SocketState},
    system::{FdEndpoint, FdRelation, SocketEndpoint, SystemSnapshot},
};
use std::collections::{HashMap, HashSet};
pub fn build(result: &mut SystemSnapshot) -> bool {
    let mut owners: HashMap<_, _> = std::mem::take(&mut result.raw_owners).into_iter().collect();
    let infos: HashMap<_, _> = std::mem::take(&mut result.raw_sockets)
        .into_iter()
        .collect();
    let inode_ns: HashMap<_, _> = std::mem::take(&mut result.raw_namespaces)
        .into_iter()
        .collect();
    let metadata: HashMap<_, _> = std::mem::take(&mut result.raw_connections)
        .into_iter()
        .collect();
    let mut truncated = false;
    let mut unknown_socket_types = 0;
    let mut unknown_connection_statuses = 0;
    for (resource, endpoints) in &mut owners {
        if resource.kind != IpcKind::Socket {
            continue;
        }
        let observed = metadata
            .get(resource)
            .copied()
            .unwrap_or(SocketConnectionStatus::Unknown);
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

    if let Some(captured) = result.raw_neighbors.take() {
        let own = captured.namespace;
        let neighbors: HashMap<_, _> = captured.entries.into_iter().collect();
        for relation in &mut result.fd_relations {
            if inode_ns.get(&relation.endpoint.resource.inode) == Some(&own)
                && let Some(socket) = &mut relation.socket
                && socket.network_peer
            {
                socket.remote_gid = socket
                    .remote
                    .and_then(|remote| neighbors.get(&remote.ip).cloned());
            }
        }
    }
    truncated
}
