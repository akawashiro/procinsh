//! Queue pair relations: owners, connection manager addresses, remote GIDs and
//! queue pairs connected within this host.
//!
//! # Interface
//! - [`build_rdma_relations`] (`pub(super)`): `fn build_rdma_relations( resources: &`[`NldevResources`]`, ports: &[`[`crate::ipc::rdma::RdmaPortInfo`]`], neighbors: &`[`HashMap`]`<`[`IpAddr`]`, `[`crate::ipc::rdma::IbGid`]`>, processes: &`[`HashMap`]`<i32, `[`ProcessId`]`>, ) -> `[`Vec`]`<`[`crate::ipc::rdma::RdmaRelation`]`>`.

use super::nldev::{CmIdEntry, NldevResources, QueuePairEntry, ResourceOwner};
use crate::ipc::schema::ProcessId;
use std::{
    collections::{HashMap, HashSet},
    net::IpAddr,
};

use crate::ipc::rdma::{
    IbGid, RdmaAddress, RdmaCmInfo, RdmaEndpoint, RdmaEndpointOwner, RdmaPortInfo, RdmaRelation,
};

struct Endpoint<'a> {
    entry: &'a QueuePairEntry,
    cm: Option<&'a CmIdEntry>,
    remote_gid: Option<IbGid>,
}
impl Endpoint<'_> {
    /// QPNs are unique per device, except the SMI and GSI QPs (0 and 1) of each port.
    fn id(&self) -> String {
        match self.entry.port {
            Some(port) => format!("rdma:{}/{port}:{}", self.entry.device, self.entry.qpn),
            None => format!("rdma:{}:{}", self.entry.device, self.entry.qpn),
        }
    }
}

fn resolve_rdma_owner(
    owner: &ResourceOwner,
    processes: &HashMap<i32, ProcessId>,
) -> RdmaEndpointOwner {
    match owner {
        ResourceOwner::Kernel(name) => RdmaEndpointOwner::Kernel { name: name.clone() },
        ResourceOwner::Process(pid) => match processes.get(&(*pid as i32)) {
            Some(id) => RdmaEndpointOwner::Process { process_id: *id },
            None => RdmaEndpointOwner::Unknown { pid: Some(*pid) },
        },
        ResourceOwner::Unknown => RdmaEndpointOwner::Unknown { pid: None },
    }
}

fn build_rdma_endpoint(
    entry: &QueuePairEntry,
    processes: &HashMap<i32, ProcessId>,
) -> RdmaEndpoint {
    RdmaEndpoint {
        owner: resolve_rdma_owner(&entry.owner, processes),
        device: entry.device.clone(),
        port: entry.port,
        qpn: entry.qpn,
        qp_type: entry.qp_type,
        state: entry.state,
    }
}

/// Whether two QPs with swapped QPNs are the two ends of one connection on this host.
/// QPNs are unique per device only, so another device needs address evidence, and
/// any contradicting address rejects the pair.
fn same_connection(a: &Endpoint, b: &Endpoint, gids: &HashMap<(&str, u32), IbGid>) -> bool {
    let local = |e: &Endpoint| {
        e.entry
            .port
            .and_then(|port| gids.get(&(e.entry.device.as_str(), port)).copied())
    };
    let gid_match = |from: &Endpoint, to: &Endpoint| match (from.remote_gid, local(to)) {
        (Some(remote), Some(local)) => Some(remote == local),
        _ => None,
    };
    let cm_match = match (a.cm, b.cm) {
        (Some(a), Some(b)) => match (a.source, a.destination, b.source, b.destination) {
            (Some(a_src), Some(a_dst), Some(b_src), Some(b_dst)) => {
                Some(a_src == b_dst && a_dst == b_src)
            }
            _ => None,
        },
        _ => None,
    };
    let evidence = [gid_match(a, b), gid_match(b, a), cm_match];
    if evidence.contains(&Some(false)) {
        return false;
    }
    a.entry.device == b.entry.device || evidence.contains(&Some(true))
}

fn format_rdma_relation_label(endpoint: &Endpoint, peer: Option<&Endpoint>) -> String {
    let entry = endpoint.entry;
    let mut label = format!("RDMA {} QP {}", entry.qp_type, entry.qpn);
    if let Some(peer) = peer {
        label += &format!(" ↔ QP {}", peer.entry.qpn);
    } else if let Some(remote) = entry.remote_qpn {
        label += &format!(" → QP {remote}");
    }
    if peer.is_none() {
        if let Some(destination) = endpoint.cm.and_then(|cm| cm.destination) {
            label += &format!(" {destination}");
        } else if let Some(gid) = endpoint.remote_gid {
            label += &format!(" {gid}");
        }
    }
    label
}

/// Builds one relation per unpaired QP and one per QP pair on this host.
/// `processes` maps PIDs of this snapshot to their identities.
pub(super) fn build_rdma_relations(
    resources: &NldevResources,
    ports: &[RdmaPortInfo],
    neighbors: &HashMap<IpAddr, IbGid>,
    processes: &HashMap<i32, ProcessId>,
) -> Vec<RdmaRelation> {
    let gids: HashMap<_, _> = ports
        .iter()
        .filter_map(|p| Some(((p.device.as_str(), p.port), p.gid?)))
        .collect();
    let mut cms: HashMap<(&str, u32), &CmIdEntry> = HashMap::new();
    for cm in &resources.cm_ids {
        if let Some(qpn) = cm.qpn {
            cms.entry((cm.device.as_str(), qpn)).or_insert(cm);
        }
    }
    let endpoints: Vec<_> = resources
        .queue_pairs
        .iter()
        .map(|entry| {
            let cm = cms.get(&(entry.device.as_str(), entry.qpn)).copied();
            let remote_gid = cm.and_then(|cm| match cm.destination? {
                RdmaAddress::Ib { gid, .. } => Some(gid),
                destination => neighbors.get(&destination.ip()?).copied(),
            });
            Endpoint {
                entry,
                cm,
                remote_gid,
            }
        })
        .collect();
    let mut by_qpn: HashMap<u32, Vec<usize>> = HashMap::new();
    for (index, e) in endpoints.iter().enumerate() {
        if e.entry.qp_type.is_connected() && e.entry.remote_qpn.is_some() {
            by_qpn.entry(e.entry.qpn).or_default().push(index);
        }
    }
    let peer_of = |index: usize| -> Option<usize> {
        let a = &endpoints[index];
        if !a.entry.qp_type.is_connected() {
            return None;
        }
        let matches: Vec<_> = by_qpn
            .get(&a.entry.remote_qpn?)?
            .iter()
            .copied()
            .filter(|&b| {
                b != index
                    && endpoints[b].entry.remote_qpn == Some(a.entry.qpn)
                    && same_connection(a, &endpoints[b], &gids)
            })
            .collect();
        (matches.len() == 1).then(|| matches[0])
    };
    let mut emitted = HashSet::new();
    let mut result = Vec::new();
    for (index, e) in endpoints.iter().enumerate() {
        if emitted.contains(&index) {
            continue;
        }
        // Pair only mutual unique matches, so each QP belongs to one relation.
        let peer = peer_of(index).filter(|&b| peer_of(b) == Some(index));
        let (first, second) = match peer {
            Some(b) if endpoints[b].id() < e.id() => (b, Some(index)),
            Some(b) => (index, Some(b)),
            None => (index, None),
        };
        emitted.insert(first);
        let first_endpoint = &endpoints[first];
        let second_endpoint = second.map(|s| {
            emitted.insert(s);
            &endpoints[s]
        });
        result.push(RdmaRelation {
            id: match second_endpoint {
                Some(peer) => format!("{}/{}", first_endpoint.id(), peer.id()),
                None => first_endpoint.id(),
            },
            endpoint: build_rdma_endpoint(first_endpoint.entry, processes),
            peer: second_endpoint.map(|peer| build_rdma_endpoint(peer.entry, processes)),
            remote_qpn: first_endpoint.entry.remote_qpn,
            cm: first_endpoint.cm.map(|cm| RdmaCmInfo {
                id: cm.id,
                owner: resolve_rdma_owner(&cm.owner, processes),
                state: cm.state,
                port_space: cm.port_space,
                source: cm.source,
                destination: cm.destination,
            }),
            remote_gid: first_endpoint.remote_gid,
            remote_hostname: None,
            label: format_rdma_relation_label(first_endpoint, second_endpoint),
        });
    }
    result
}

/// An unpaired user QP for encoder tests.
#[cfg(test)]
pub(in crate::privileged_helper::system) fn test_relation(qpn: u32) -> RdmaRelation {
    use crate::ipc::rdma::{RdmaQpState, RdmaQpType};
    RdmaRelation {
        id: format!("rdma:ib0/1:{qpn}"),
        endpoint: RdmaEndpoint {
            owner: RdmaEndpointOwner::Unknown { pid: Some(1) },
            device: "ib0".into(),
            port: Some(1),
            qpn,
            qp_type: RdmaQpType::Rc,
            state: RdmaQpState::Rts,
        },
        peer: None,
        remote_qpn: Some(qpn + 1000),
        cm: None,
        remote_gid: None,
        remote_hostname: None,
        label: format!("RDMA RC QP {qpn}"),
    }
}

#[cfg(test)]
mod tests {
    use {
        super::*,
        crate::ipc::rdma::{
            IbGid, RdmaAddress, RdmaCmState, RdmaPortInfo, RdmaPortSpace, RdmaQpState, RdmaQpType,
        },
    };

    fn queue_pair_fixture(
        device: &str,
        qpn: u32,
        remote: Option<u32>,
        owner: ResourceOwner,
    ) -> QueuePairEntry {
        QueuePairEntry {
            device: device.into(),
            port: Some(1),
            qpn,
            remote_qpn: remote,
            qp_type: RdmaQpType::Rc,
            state: RdmaQpState::Rts,
            owner,
        }
    }

    fn connection_id_fixture(device: &str, qpn: u32, source: &str, destination: &str) -> CmIdEntry {
        let address = |text: &str| {
            let address: std::net::SocketAddr = text.parse().unwrap();
            RdmaAddress::Inet {
                ip: address.ip(),
                port: address.port(),
            }
        };
        CmIdEntry {
            device: device.into(),
            id: Some(qpn),
            qpn: Some(qpn),
            state: RdmaCmState::Connect,
            port_space: RdmaPortSpace::Tcp,
            source: Some(address(source)),
            destination: Some(address(destination)),
            owner: ResourceOwner::Kernel("rpcrdma".into()),
        }
    }

    fn rdma_port_fixture(device: &str, gid: &str) -> RdmaPortInfo {
        RdmaPortInfo {
            device: device.into(),
            port: 1,
            link_layer: Some("InfiniBand".into()),
            state: Some("ACTIVE".into()),
            lid: Some(1),
            gid: IbGid::parse(gid),
            rate: None,
        }
    }

    const LOCAL: &str = "fe80:0000:0000:0000:0002:c903:0028:45c7";
    const REMOTE: &str = "fe80:0000:0000:0000:f452:1403:0080:de51";

    #[test]
    fn remote_qps_keep_cm_destination_and_neighbor_gid() {
        let resources = NldevResources {
            queue_pairs: vec![
                queue_pair_fixture(
                    "ib0",
                    524,
                    Some(650),
                    ResourceOwner::Kernel("rdma_cm".into()),
                ),
                queue_pair_fixture("ib0", 7, None, ResourceOwner::Process(99)),
                queue_pair_fixture("ib0", 8, None, ResourceOwner::Process(42)),
            ],
            cm_ids: vec![connection_id_fixture(
                "ib0",
                524,
                "10.10.10.2:58985",
                "10.10.10.1:20049",
            )],
            ..Default::default()
        };
        let neighbors =
            HashMap::from([("10.10.10.1".parse().unwrap(), IbGid::parse(REMOTE).unwrap())]);
        let processes = HashMap::from([(
            42,
            ProcessId {
                pid: 42,
                start_time_ticks: 7,
            },
        )]);
        let result = build_rdma_relations(
            &resources,
            &[rdma_port_fixture("ib0", LOCAL)],
            &neighbors,
            &processes,
        );
        assert_eq!(result.len(), 3);
        let json = serde_json::to_value(&result[0]).unwrap();
        assert_eq!(json["id"], "rdma:ib0/1:524");
        assert_eq!(json["peer"], serde_json::Value::Null);
        assert_eq!(json["remote_qpn"], 650);
        assert_eq!(json["remote_gid"], REMOTE);
        assert_eq!(
            json["endpoint"]["owner"],
            serde_json::json!({"kind":"kernel","name":"rdma_cm"})
        );
        assert_eq!(
            json["endpoint"]["qp_type"],
            serde_json::json!({"kind":"rc"})
        );
        assert_eq!(
            json["cm"]["owner"],
            serde_json::json!({"kind":"kernel","name":"rpcrdma"})
        );
        assert_eq!(
            json["cm"]["destination"],
            serde_json::json!({"kind":"inet","ip":"10.10.10.1","port":20049})
        );
        assert_eq!(json["label"], "RDMA RC QP 524 → QP 650 10.10.10.1:20049");
        assert_eq!(
            serde_json::to_value(&result[1].endpoint.owner).unwrap(),
            serde_json::json!({"kind":"unknown","pid":99})
        );
        assert_eq!(
            serde_json::to_value(&result[2].endpoint.owner).unwrap(),
            serde_json::json!({"kind":"process","process_id":{"pid":42,"start_time_ticks":7}})
        );
    }

    #[test]
    fn local_pairs_need_unique_swaps_and_no_contradicting_addresses() {
        let process_owner_fixture = || ResourceOwner::Process(1);
        // Same device: a QPN swap identifies a loopback connection.
        let same = NldevResources {
            queue_pairs: vec![
                queue_pair_fixture("ib0", 20, Some(10), process_owner_fixture()),
                queue_pair_fixture("ib0", 10, Some(20), process_owner_fixture()),
            ],
            ..Default::default()
        };
        let result = build_rdma_relations(&same, &[], &HashMap::new(), &HashMap::new());
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, "rdma:ib0/1:10/rdma:ib0/1:20");
        assert_eq!(result[0].peer.as_ref().unwrap().qpn, 20);
        assert_eq!(result[0].label, "RDMA RC QP 10 ↔ QP 20");
        // Different devices without address evidence remain separate.
        let devices = NldevResources {
            queue_pairs: vec![
                queue_pair_fixture("ib0", 10, Some(20), process_owner_fixture()),
                queue_pair_fixture("ib1", 20, Some(10), process_owner_fixture()),
            ],
            ..Default::default()
        };
        assert_eq!(
            build_rdma_relations(&devices, &[], &HashMap::new(), &HashMap::new()).len(),
            2
        );
        // Swapped CM addresses confirm a pair across devices.
        let confirmed = NldevResources {
            cm_ids: vec![
                connection_id_fixture("ib0", 10, "10.0.0.1:1", "10.0.0.2:2"),
                connection_id_fixture("ib1", 20, "10.0.0.2:2", "10.0.0.1:1"),
            ],
            ..devices
        };
        assert_eq!(
            build_rdma_relations(&confirmed, &[], &HashMap::new(), &HashMap::new()).len(),
            1
        );
        // A remote GID naming another port contradicts a same-device QPN swap.
        let contradicted = NldevResources {
            cm_ids: vec![connection_id_fixture("ib0", 10, "10.0.0.1:1", "10.0.0.9:2")],
            ..same
        };
        let neighbors =
            HashMap::from([("10.0.0.9".parse().unwrap(), IbGid::parse(REMOTE).unwrap())]);
        let result = build_rdma_relations(
            &contradicted,
            &[rdma_port_fixture("ib0", LOCAL)],
            &neighbors,
            &HashMap::new(),
        );
        assert_eq!(result.len(), 2);
        // An ambiguous swap is not paired.
        let ambiguous = NldevResources {
            queue_pairs: vec![
                queue_pair_fixture("ib0", 10, Some(20), process_owner_fixture()),
                queue_pair_fixture("ib1", 20, Some(10), process_owner_fixture()),
                queue_pair_fixture("ib2", 20, Some(10), process_owner_fixture()),
            ],
            cm_ids: vec![
                connection_id_fixture("ib0", 10, "10.0.0.1:1", "10.0.0.2:2"),
                connection_id_fixture("ib1", 20, "10.0.0.2:2", "10.0.0.1:1"),
                connection_id_fixture("ib2", 20, "10.0.0.2:2", "10.0.0.1:1"),
            ],
            ..Default::default()
        };
        assert_eq!(
            build_rdma_relations(&ambiguous, &[], &HashMap::new(), &HashMap::new()).len(),
            3
        );
    }
}
