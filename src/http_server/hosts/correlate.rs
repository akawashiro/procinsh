//! Connections whose two ends are observed on different hosts.

//!
//! Each host reports a connection to another host as an external endpoint. A TCP
//! or UDP pair matches by reversed address tuples; an RDMA pair by swapped QPNs
//! with remote GID or connection manager addresses as evidence. Only unique
//! matches link, so NAT and duplicated tuples stay external.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::HashMap, net::IpAddr};

/// One end of a cross-host link: a host and a relation ID in its snapshot.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub(in crate::http_server) struct LinkEnd {
    pub(in crate::http_server) host: String,
    pub(in crate::http_server) relation: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(in crate::http_server) struct SocketLink {
    pub(in crate::http_server) a: LinkEnd,
    pub(in crate::http_server) b: LinkEnd,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(in crate::http_server) struct RdmaEvidence {
    /// Each remote GID equals the other end's port GID.
    pub(in crate::http_server) gid: bool,
    /// The connection manager addresses are swapped.
    pub(in crate::http_server) cm: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(in crate::http_server) struct RdmaLink {
    pub(in crate::http_server) a: LinkEnd,
    pub(in crate::http_server) b: LinkEnd,
    pub(in crate::http_server) evidence: RdmaEvidence,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub(in crate::http_server) struct CrossHostLinks {
    pub(in crate::http_server) sockets: Vec<SocketLink>,
    pub(in crate::http_server) rdma: Vec<RdmaLink>,
}

#[derive(Deserialize)]
struct Kind {
    kind: String,
}
#[derive(Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
struct Address {
    ip: IpAddr,
    port: u16,
}
#[derive(Deserialize)]
struct Socket {
    protocol: Kind,
    local: Option<Address>,
    remote: Option<Address>,
    network_peer: bool,
}
#[derive(Deserialize)]
struct FdRelation {
    id: String,
    peer: Option<Value>,
    shared: bool,
    socket: Option<Socket>,
}
#[derive(Deserialize)]
struct RdmaEndpoint {
    device: String,
    port: Option<u32>,
    qpn: u32,
    qp_type: Kind,
}
#[derive(Deserialize)]
struct CmInfo {
    source: Option<Value>,
    destination: Option<Value>,
}
#[derive(Deserialize)]
struct RdmaRelation {
    id: String,
    endpoint: RdmaEndpoint,
    peer: Option<Value>,
    remote_qpn: Option<u32>,
    cm: Option<CmInfo>,
    remote_gid: Option<String>,
}
#[derive(Deserialize)]
struct RdmaPort {
    device: String,
    port: u32,
    link_layer: Option<String>,
    gid: Option<String>,
}

fn entries<T: for<'de> Deserialize<'de>>(snapshot: &Value, name: &str) -> Vec<T> {
    // Entries that do not fit the expected shape are skipped, not fatal.
    snapshot
        .get(name)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|entry| T::deserialize(entry).ok())
        .collect()
}

fn routable(address: Address) -> bool {
    address.port != 0 && !address.ip.is_unspecified() && !address.ip.is_loopback()
}

fn socket_links(hosts: &[(&str, &Value)]) -> Vec<SocketLink> {
    type Tuple = (String, Address, Address);
    let mut by_tuple: HashMap<Tuple, Vec<LinkEnd>> = HashMap::new();
    for (host, snapshot) in hosts {
        for relation in entries::<FdRelation>(snapshot, "fd_relations") {
            let Some(socket) = relation.socket else {
                continue;
            };
            if relation.peer.is_some() || relation.shared || !socket.network_peer {
                continue;
            }
            let (Some(local), Some(remote)) = (socket.local, socket.remote) else {
                continue;
            };
            if !routable(local) || !routable(remote) {
                continue;
            }
            by_tuple
                .entry((socket.protocol.kind, local, remote))
                .or_default()
                .push(LinkEnd {
                    host: (*host).to_owned(),
                    relation: relation.id,
                });
        }
    }
    let mut links = Vec::new();
    for ((kind, local, remote), ends) in &by_tuple {
        let Some(reverse) = by_tuple.get(&(kind.clone(), *remote, *local)) else {
            continue;
        };
        // Each tuple once, from its smaller end; both sides must be unique.
        if ends.len() != 1 || reverse.len() != 1 || ends[0] >= reverse[0] {
            continue;
        }
        if ends[0].host != reverse[0].host {
            links.push(SocketLink {
                a: ends[0].clone(),
                b: reverse[0].clone(),
            });
        }
    }
    links.sort_by(|x, y| (&x.a, &x.b).cmp(&(&y.a, &y.b)));
    links
}

struct Qp<'a> {
    end: LinkEnd,
    relation: RdmaRelation,
    gid: Option<&'a str>,
    link_layer: Option<&'a str>,
}

fn rdma_links(hosts: &[(&str, &Value)]) -> Vec<RdmaLink> {
    let ports: Vec<Vec<RdmaPort>> = hosts
        .iter()
        .map(|(_, snapshot)| entries(snapshot, "rdma_ports"))
        .collect();
    let mut qps = Vec::new();
    for ((host, snapshot), ports) in hosts.iter().zip(&ports) {
        for relation in entries::<RdmaRelation>(snapshot, "rdma_relations") {
            let connected = matches!(
                relation.endpoint.qp_type.kind.as_str(),
                "rc" | "uc" | "xrc_ini" | "xrc_tgt"
            );
            if relation.peer.is_some() || !connected || relation.remote_qpn.is_none() {
                continue;
            }
            let port = ports.iter().find(|p| {
                p.device == relation.endpoint.device && Some(p.port) == relation.endpoint.port
            });
            qps.push(Qp {
                end: LinkEnd {
                    host: (*host).to_owned(),
                    relation: relation.id.clone(),
                },
                relation,
                gid: port.and_then(|p| p.gid.as_deref()),
                link_layer: port.and_then(|p| p.link_layer.as_deref()),
            });
        }
    }
    let mut by_qpn: HashMap<u32, Vec<usize>> = HashMap::new();
    for (index, qp) in qps.iter().enumerate() {
        by_qpn
            .entry(qp.relation.endpoint.qpn)
            .or_default()
            .push(index);
    }
    let evidence = |a: &Qp, b: &Qp| -> Option<RdmaEvidence> {
        let gid = |from: &Qp, to: &Qp| match (from.relation.remote_gid.as_deref(), to.gid) {
            (Some(remote), Some(local)) => Some(remote == local),
            _ => None,
        };
        let cm = match (&a.relation.cm, &b.relation.cm) {
            (Some(x), Some(y)) => match (&x.source, &x.destination, &y.source, &y.destination) {
                (Some(xs), Some(xd), Some(ys), Some(yd)) => Some(xs == yd && xd == ys),
                _ => None,
            },
            _ => None,
        };
        let gids = [gid(a, b), gid(b, a)];
        if gids.contains(&Some(false)) || cm == Some(false) {
            return None;
        }
        Some(RdmaEvidence {
            gid: gids.contains(&Some(true)),
            cm: cm == Some(true),
        })
    };
    let candidates = |index: usize| -> Vec<(usize, RdmaEvidence)> {
        let a = &qps[index];
        by_qpn
            .get(&a.relation.remote_qpn.unwrap())
            .into_iter()
            .flatten()
            .filter(|&&b| {
                qps[b].end.host != a.end.host
                    && qps[b].relation.remote_qpn == Some(a.relation.endpoint.qpn)
            })
            .filter_map(|&b| Some((b, evidence(a, &qps[b])?)))
            .collect()
    };
    let mut links = Vec::new();
    for index in 0..qps.len() {
        let found = candidates(index);
        let [(other, evidence)] = found.as_slice() else {
            continue;
        };
        let (a, b) = (&qps[index], &qps[*other]);
        // A bare QPN swap links only when unique in both directions on one link layer.
        let unique = matches!(candidates(*other).as_slice(), [(back, _)] if *back == index);
        let addressed = evidence.gid || evidence.cm;
        if a.end >= b.end || !unique || (!addressed && a.link_layer != b.link_layer) {
            continue;
        }
        links.push(RdmaLink {
            a: a.end.clone(),
            b: b.end.clone(),
            evidence: *evidence,
        });
    }
    links.sort_by(|x, y| (&x.a, &x.b).cmp(&(&y.a, &y.b)));
    links
}

/// Links between `hosts`, given as host names and their snapshots.
pub(in crate::http_server) fn correlate(hosts: &[(&str, &Value)]) -> CrossHostLinks {
    CrossHostLinks {
        sockets: socket_links(hosts),
        rdma: rdma_links(hosts),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tcp(id: &str, local: &str, remote: &str) -> Value {
        let address = |text: &str| {
            let a: std::net::SocketAddr = text.parse().unwrap();
            json!({"ip": a.ip(), "port": a.port()})
        };
        json!({"id": id, "peer": null, "shared": false, "socket": {
            "protocol": {"kind": "tcp", "family": "ipv4"}, "state": {"kind": "established"},
            "local": address(local), "remote": address(remote), "network_peer": true}})
    }

    fn qp(id: &str, qpn: u32, remote: u32, gid: Option<&str>, cm: Option<(&str, &str)>) -> Value {
        let inet = |text: &str| {
            let a: std::net::SocketAddr = text.parse().unwrap();
            json!({"kind": "inet", "ip": a.ip(), "port": a.port()})
        };
        json!({"id": id, "peer": null, "remote_qpn": remote, "remote_gid": gid,
            "endpoint": {"device": "ib0", "port": 1, "qpn": qpn, "qp_type": {"kind": "rc"}},
            "cm": cm.map(|(s, d)| json!({"source": inet(s), "destination": inet(d)}))})
    }

    fn host<'a>(name: &'a str, fd: Vec<Value>, rdma: Vec<Value>, gid: &str) -> (&'a str, Value) {
        (
            name,
            json!({"fd_relations": fd, "rdma_relations": rdma,
                "rdma_ports": [{"device": "ib0", "port": 1, "link_layer": "InfiniBand", "gid": gid}]}),
        )
    }

    fn correlate(hosts: &[(&str, Value)]) -> CrossHostLinks {
        let hosts: Vec<_> = hosts.iter().map(|(name, s)| (*name, s)).collect();
        super::correlate(&hosts)
    }

    const GA: &str = "fe80:0000:0000:0000:0000:0000:0000:000a";
    const GB: &str = "fe80:0000:0000:0000:0000:0000:0000:000b";

    #[test]
    fn reversed_unique_tuples_link_sockets_across_hosts() {
        let links = correlate(&[
            host(
                "a",
                vec![
                    tcp("x", "10.0.0.1:5000", "10.0.0.2:443"),
                    tcp("dup1", "10.0.0.1:1", "10.0.0.9:9"),
                ],
                vec![],
                GA,
            ),
            host(
                "b",
                vec![
                    tcp("y", "10.0.0.2:443", "10.0.0.1:5000"),
                    tcp("dup2", "10.0.0.9:9", "10.0.0.1:1"),
                    tcp("lo", "127.0.0.1:80", "127.0.0.1:81"),
                ],
                vec![],
                GB,
            ),
            host(
                "c",
                vec![
                    tcp("dup3", "10.0.0.9:9", "10.0.0.1:1"),
                    tcp("lo2", "127.0.0.1:81", "127.0.0.1:80"),
                ],
                vec![],
                GA,
            ),
        ]);
        assert_eq!(
            serde_json::to_value(&links.sockets).unwrap(),
            json!([{"a": {"host": "a", "relation": "x"}, "b": {"host": "b", "relation": "y"}}])
        );
        // The same host never links to itself, even with reversed tuples.
        let single = correlate(&[host(
            "a",
            vec![
                tcp("x", "10.0.0.1:1", "10.0.0.2:2"),
                tcp("y", "10.0.0.2:2", "10.0.0.1:1"),
            ],
            vec![],
            GA,
        )]);
        assert!(single.sockets.is_empty());
    }

    #[test]
    fn rdma_links_need_swapped_qpns_and_consistent_addresses() {
        // Remote GIDs naming each other's ports.
        let links = correlate(&[
            host("a", vec![], vec![qp("qa", 10, 20, Some(GB), None)], GA),
            host("b", vec![], vec![qp("qb", 20, 10, Some(GA), None)], GB),
        ]);
        assert_eq!(
            serde_json::to_value(&links.rdma).unwrap(),
            json!([{"a": {"host": "a", "relation": "qa"}, "b": {"host": "b", "relation": "qb"},
                "evidence": {"gid": true, "cm": false}}])
        );
        // Swapped CM addresses, and a unique bare swap on the same link layer.
        let cm = correlate(&[
            host(
                "a",
                vec![],
                vec![qp("qa", 10, 20, None, Some(("10.0.0.1:1", "10.0.0.2:2")))],
                GA,
            ),
            host(
                "b",
                vec![],
                vec![qp("qb", 20, 10, None, Some(("10.0.0.2:2", "10.0.0.1:1")))],
                GB,
            ),
        ]);
        assert_eq!(
            cm.rdma[0].evidence,
            RdmaEvidence {
                gid: false,
                cm: true
            }
        );
        let bare = correlate(&[
            host("a", vec![], vec![qp("qa", 10, 20, None, None)], GA),
            host("b", vec![], vec![qp("qb", 20, 10, None, None)], GB),
        ]);
        assert_eq!(bare.rdma.len(), 1);
        // A remote GID of a third machine contradicts the swap.
        let contradicted = correlate(&[
            host(
                "a",
                vec![],
                vec![qp(
                    "qa",
                    10,
                    20,
                    Some("fe80:0000:0000:0000:0000:0000:0000:00ff"),
                    None,
                )],
                GA,
            ),
            host("b", vec![], vec![qp("qb", 20, 10, None, None)], GB),
        ]);
        assert!(contradicted.rdma.is_empty());
        // An ambiguous bare swap stays external.
        let ambiguous = correlate(&[
            host("a", vec![], vec![qp("qa", 10, 20, None, None)], GA),
            host("b", vec![], vec![qp("qb", 20, 10, None, None)], GB),
            host("c", vec![], vec![qp("qc", 20, 10, None, None)], GB),
        ]);
        assert!(ambiguous.rdma.is_empty());
        // Malformed entries are ignored.
        let malformed = correlate(&[host("a", vec![json!({"id": 1})], vec![json!("x")], GA)]);
        assert_eq!(malformed, CrossHostLinks::default());
    }
}
