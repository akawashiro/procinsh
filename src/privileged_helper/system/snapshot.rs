//! Acquire process structure and raw descriptor observations for main-side interpretation.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`process_from_summary`] | `pub(super)` | `fn process_from_summary( summary: `[`ProcessSummary`]`, parent_id: `[`Option`]`<`[`ProcessId`]`>, ) -> `[`ProcessSnapshot`] |
//! | [`collect_system_snapshot`] | `pub(super)` | `fn collect_system_snapshot() -> `[`SystemSnapshot`] |
//! | [`collect_system_snapshot_from_sources`] | `pub(super)` | `fn collect_system_snapshot_from_sources(sources: &`[`HardwareSources`]`) -> `[`SystemSnapshot`] |

#[cfg(not(test))]
use super::hardware::HardwareSources;
use super::rdma;
#[cfg(test)]
use crate::http_server::analysis::hardware::observation::HardwareSources;
use crate::ipc::model::resource::{IpcIdentity, IpcKind};
use crate::ipc::model::socket_types::FdKind;
#[cfg(test)]
use crate::ipc::model::socket_types::SocketState;
use crate::ipc::model::socket_types::{
    AddressFamily, SocketConnectionStatus, SocketProtocol, SocketType,
};
#[cfg(test)]
use crate::ipc::model::system::FdRelation;
use crate::ipc::model::system::{FdEndpoint, NeighborSnapshot, ProcessSnapshot, SystemSnapshot};
use crate::{
    ipc::{acquisition::ProcessSummary, model::sockets::SocketInfo, schema::ProcessId},
    privileged_helper::process,
};
use std::{
    collections::{HashMap, HashSet},
    fs,
    os::unix::fs::{FileTypeExt, MetadataExt},
    time::{Duration, Instant},
};

/// Adds IPoIB remote GIDs to network destinations of the collector's own network
/// namespace. Neighbors are read only when an IPoIB interface exists.
fn add_remote_gids(
    result: &mut SystemSnapshot,
    _inode_ns: &HashMap<u64, (u64, u64)>,
    sources: &HardwareSources,
) {
    if !super::ipoib::has_ipoib_interface(&sources.sys) {
        return;
    }
    let Ok(own) = fs::metadata("/proc/thread-self/ns/net") else {
        return;
    };
    let neighbors = match super::ipoib::ipoib_gids(Instant::now() + Duration::from_millis(200)) {
        Ok(neighbors) => neighbors,
        Err(error) => {
            result
                .warnings
                .push(format!("IPoIB neighbors unavailable: {error:#}"));
            return;
        }
    };
    result.raw_neighbors = Some(NeighborSnapshot {
        entries: neighbors
            .iter()
            .map(|(ip, gid)| (*ip, gid.clone()))
            .collect(),
        namespace: (own.dev(), own.ino()),
    });
    #[cfg(test)]
    apply_remote_gids(
        &mut result.fd_relations,
        &neighbors,
        _inode_ns,
        (own.dev(), own.ino()),
    );
}

/// Sets `remote_gid` of network destinations whose socket is in namespace `own`.
#[cfg(test)]
fn apply_remote_gids(
    relations: &mut [FdRelation],
    neighbors: &HashMap<std::net::IpAddr, String>,
    inode_ns: &HashMap<u64, (u64, u64)>,
    own: (u64, u64),
) {
    for relation in relations {
        let same_namespace = inode_ns.get(&relation.endpoint.resource.inode) == Some(&own);
        if let Some(socket) = &mut relation.socket
            && socket.network_peer
            && same_namespace
        {
            socket.remote_gid = socket.remote.and_then(|r| neighbors.get(&r.ip).cloned());
        }
    }
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

pub(super) fn collect_system_snapshot() -> SystemSnapshot {
    collect_system_snapshot_from_sources(&HardwareSources::default())
}

/// Collects with hardware read from `sources`, which tests point at fixtures.
pub(super) fn collect_system_snapshot_from_sources(sources: &HardwareSources) -> SystemSnapshot {
    #[cfg(test)]
    use crate::http_server::analysis::hardware::finish_device_handle_collector;
    #[cfg(test)]
    use crate::http_server::analysis::hardware::observation::acquired_hardware_sources;
    #[cfg(test)]
    use crate::http_server::analysis::hardware::observation::handles_hardware_observation;
    #[cfg(test)]
    use crate::http_server::analysis::hardware::observation::observe_hardware_observation;
    #[cfg(test)]
    use crate::http_server::analysis::hardware::observe_device_handle_collector;
    use {
        crate::ipc::model::resource::device_id_from_stat,
        crate::ipc::model::socket_types::fd_access_from_flags, crate::ipc::rdma::ip_rdma_address,
        crate::privileged_helper::process::create_socket_metadata_reader,
        crate::privileged_helper::process::read_socket_metadata,
        crate::privileged_helper::process::socket_metadata_has_observation,
        crate::privileged_helper::system::resolver::create_reverse_dns_resolver,
        crate::privileged_helper::system::resolver::lookup_reverse_dns_resolver,
    };

    let mut result = SystemSnapshot {
        captured_at: process::timestamp_ms(),
        ..Default::default()
    };
    #[cfg(not(test))]
    {
        result.raw_hardware =
            super::hardware_capture::collect_hardware_files(&sources.sys, &sources.proc);
    }
    #[cfg(test)]
    {
        result.raw_hardware =
            super::acquisition::collect_hardware_files(&sources.sys, &sources.proc);
    }
    if result.raw_hardware.truncated {
        result
            .warnings
            .push("Hardware acquisition size limit reached".into());
    }
    #[cfg(test)]
    let acquired = acquired_hardware_sources(result.raw_hardware.clone());
    #[cfg(test)]
    let hardware = observe_hardware_observation(&acquired);
    #[cfg(test)]
    let mut handles = handles_hardware_observation(&hardware, &acquired);
    let summaries = match process::collect_processes() {
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
    let mut network_namespaces = Vec::new();
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
        let mut metadata_reader = create_socket_metadata_reader(n.identity);
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
            match process::open_network_namespace(n.identity) {
                Ok(namespace) => network_namespaces.push((n.identity, namespace)),
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
                        if meta.file_type().is_char_device() {
                            result
                                .raw_device_handles
                                .push((n.identity, number, meta.rdev()));
                            #[cfg(test)]
                            observe_device_handle_collector(
                                &mut handles,
                                n.identity,
                                number,
                                meta.rdev(),
                            );
                        }
                        continue;
                    };
                    let resource = IpcIdentity {
                        kind: if kind == FdKind::Pipe {
                            IpcKind::Pipe
                        } else {
                            IpcKind::Socket
                        },
                        device: device_id_from_stat(meta.dev()),
                        inode: meta.ino(),
                    };
                    if kind == FdKind::Socket
                        && !metadata.contains_key(&resource)
                        && let Ok(observed) = read_socket_metadata(
                            &mut metadata_reader,
                            number,
                            meta.dev(),
                            meta.ino(),
                        )
                        && socket_metadata_has_observation(&observed)
                    {
                        metadata.insert(resource, observed);
                    }
                    let flags =
                        process::read_procfs_fields(&format!("/proc/{pid}/fdinfo/{number}"))
                            .ok()
                            .and_then(|f| {
                                f.get("flags").and_then(|v| u32::from_str_radix(v, 8).ok())
                            })
                            .unwrap_or(u32::MAX);
                    endpoints.push(FdEndpoint {
                        process_id: n.identity,
                        fd: number,
                        fd_count: 1,
                        resource,
                        connection_status: (kind == FdKind::Socket)
                            .then_some(SocketConnectionStatus::Unknown),
                        kind,
                        access: fd_access_from_flags(flags),
                    });
                }
            }
            Err(e) => {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    denied += 1;
                }
            }
        }
        if process::validate_process_identity(n.identity).is_ok() {
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
    for (id, namespace) in network_namespaces {
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
                (namespace.device, namespace.inode)
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
    result.raw_owners = owners.into_iter().collect();
    let resolver = {
        static RESOLVER: std::sync::OnceLock<super::resolver::ReverseDnsResolver> =
            std::sync::OnceLock::new();
        RESOLVER.get_or_init(create_reverse_dns_resolver)
    };
    let ips: HashSet<_> = infos
        .values()
        .filter_map(|socket| socket.remote.map(|address| address.ip()))
        .collect();
    result.raw_hostnames = ips
        .into_iter()
        .filter_map(|ip| lookup_reverse_dns_resolver(resolver, ip).map(|name| (ip, name)))
        .collect();
    result.raw_sockets = infos.into_iter().collect();
    result.raw_namespaces = inode_ns.iter().map(|(inode, ns)| (*inode, *ns)).collect();
    result.raw_connections = metadata
        .into_iter()
        .map(|(id, meta)| (id, meta.connection_status))
        .collect();
    #[cfg(test)]
    {
        truncated |= crate::http_server::analysis::relations::build_ipc_relations(&mut result);
    }
    add_remote_gids(&mut result, &inode_ns, sources);
    let pids = result
        .processes
        .iter()
        .map(|p| (p.identity.pid, p.identity))
        .collect();
    let observed = rdma::collect_rdma_observation(
        &sources.sys,
        &pids,
        Instant::now() + Duration::from_millis(300),
    );
    result.rdma_ports = observed.ports;
    result.rdma_relations = observed.relations;
    for relation in &mut result.rdma_relations {
        if relation.peer.is_none() {
            relation.remote_hostname = relation
                .cm
                .as_ref()
                .and_then(|cm| ip_rdma_address(&(cm.destination?)))
                .and_then(|ip| lookup_reverse_dns_resolver(resolver, ip));
        }
    }
    result.warnings.extend(observed.warnings);
    #[cfg(test)]
    {
        let live = result.processes.iter().map(|p| p.identity).collect();
        result.device_handles = finish_device_handle_collector(handles, &live);
        result.hardware_devices = hardware.devices;
        result.hardware_paths = hardware.paths;
    }
    if denied > 0 {
        result
            .warnings
            .push(format!("Access denied for {denied} processes/FDs"));
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
    use {
        super::*,
        crate::ipc::model::{
            socket_types::FdAccess,
            system::{FdEndpoint, FdRelation, SocketEndpoint},
        },
    };

    #[test]
    fn remote_gids_apply_only_to_destinations_of_the_collector_namespace() {
        use crate::ipc::model::resource::device_id_from_stat;

        use crate::ipc::model::sockets::SocketInfo;
        let info = SocketInfo {
            protocol: SocketProtocol::Tcp {
                family: AddressFamily::Ipv4,
            },
            state: Some(SocketState::Established),
            local: Some("10.10.10.2:5000".parse().unwrap()),
            remote: Some("10.10.10.1:20049".parse().unwrap()),
            path: None,
            peer_inode: None,
        };
        let relation = |inode: u64| FdRelation {
            id: inode.to_string(),
            endpoint: FdEndpoint {
                process_id: ProcessId {
                    pid: 1,
                    start_time_ticks: 1,
                },
                fd: 3,
                fd_count: 1,
                resource: IpcIdentity {
                    kind: IpcKind::Socket,
                    device: device_id_from_stat(0),
                    inode,
                },
                kind: FdKind::Socket,
                access: FdAccess::ReadWrite,
                connection_status: None,
            },
            peer: None,
            label: String::new(),
            socket: Some(SocketEndpoint::from(&info)),
            candidate: false,
            shared: false,
        };
        let mut relations = [relation(1), relation(2)];
        let neighbors = HashMap::from([(
            "10.10.10.1".parse().unwrap(),
            "fe80:0000:0000:0000:f452:1403:0080:de51".to_owned(),
        )]);
        let namespaces = HashMap::from([(1, (5, 100)), (2, (5, 200))]);
        apply_remote_gids(&mut relations, &neighbors, &namespaces, (5, 100));
        let gid = |r: &FdRelation| r.socket.as_ref().unwrap().remote_gid.clone();
        assert_eq!(
            gid(&relations[0]).as_deref(),
            Some("fe80:0000:0000:0000:f452:1403:0080:de51")
        );
        assert_eq!(
            gid(&relations[1]),
            None,
            "another namespace has its own neighbors"
        );
        assert_eq!(
            serde_json::to_value(relations[1].socket.as_ref().unwrap()).unwrap()["remote_gid"],
            serde_json::Value::Null
        );
    }

    #[test]
    fn socket_destination_classification() {
        use crate::ipc::model::socket_types::socket_protocol_from_codes;

        use crate::ipc::model::sockets::SocketInfo;
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
        info.protocol = socket_protocol_from_codes(
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
