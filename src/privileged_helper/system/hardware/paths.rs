//! PCIe paths between devices, classified like `nvidia-smi topo -m`.

use super::pci::{HardwareRole, PciNode, PcieLink};
use serde::Serialize;
use std::collections::HashMap;

/// How a path crosses the topology, from the closest to the farthest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::privileged_helper::system) enum PathClass {
    /// At most one PCIe switch (or functions of one device).
    Pix,
    /// Several PCIe switches without the host bridge.
    Pxb,
    /// Through the host bridge of one root complex.
    Phb,
    /// Between host bridges of one NUMA node.
    Node,
    /// Between NUMA nodes.
    Sys,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(in crate::privileged_helper::system) struct HardwarePath {
    pub(in crate::privileged_helper::system) id: String,
    pub(in crate::privileged_helper::system) a: String,
    pub(in crate::privileged_helper::system) b: String,
    pub(in crate::privileged_helper::system) class: PathClass,
    /// Nodes between the ends, in order from `a` to `b`.
    pub(in crate::privileged_helper::system) via: Vec<String>,
    /// The slowest negotiated link on the path; null when no link speed is known.
    pub(in crate::privileged_helper::system) bottleneck: Option<PcieLink>,
}

fn chain<'a>(node: &'a PciNode, nodes: &HashMap<&str, &'a PciNode>) -> Vec<&'a PciNode> {
    let mut chain = vec![node];
    while let Some(parent) = chain
        .last()
        .unwrap()
        .parent
        .as_deref()
        .and_then(|p| nodes.get(p))
    {
        chain.push(parent);
    }
    chain.reverse();
    chain
}

/// Paths between every pair of endpoints. With one online NUMA node, devices
/// without a NUMA node share it.
pub(in crate::privileged_helper::system) fn paths(
    nodes: &[PciNode],
    numa_nodes: usize,
) -> Vec<HardwarePath> {
    let by_address: HashMap<&str, &PciNode> =
        nodes.iter().map(|n| (n.address.as_str(), n)).collect();
    let endpoints: Vec<_> = nodes
        .iter()
        .filter(|n| n.role == HardwareRole::Endpoint)
        .collect();
    let numa = |node: &PciNode| node.numa_node.or((numa_nodes <= 1).then_some(0));
    let mut result = Vec::new();
    for (i, a) in endpoints.iter().enumerate() {
        for b in &endpoints[i + 1..] {
            let (ca, cb) = (chain(a, &by_address), chain(b, &by_address));
            let common = ca
                .iter()
                .zip(&cb)
                .take_while(|(x, y)| x.address == y.address)
                .count();
            // Links climbed from each end up to the common ancestor or host bridge.
            let (up, down) = (&ca[common.max(1)..], &cb[common.max(1)..]);
            // From a's parent up to the common ancestor (or a's host bridge), then down to b's parent.
            let mut via: Vec<String> = Vec::new();
            let top = common.saturating_sub(1);
            via.extend(
                ca[top..ca.len() - 1]
                    .iter()
                    .rev()
                    .map(|n| n.address.clone()),
            );
            let below = if common == 0 { 0 } else { common };
            via.extend(cb[below..cb.len() - 1].iter().map(|n| n.address.clone()));
            let class = if common == 0 {
                if numa(a).is_some() && numa(a) == numa(b) {
                    PathClass::Node
                } else {
                    PathClass::Sys
                }
            } else if ca[common - 1].role == HardwareRole::HostBridge {
                PathClass::Phb
            } else {
                // A switch is an upstream port and its downstream ports.
                let mut switches: Vec<&str> = Vec::new();
                for node in ca[common - 1..ca.len() - 1]
                    .iter()
                    .chain(&cb[common..cb.len() - 1])
                {
                    if node.role != HardwareRole::SwitchPort {
                        continue;
                    }
                    let parent = node.parent.as_deref().and_then(|p| by_address.get(p));
                    let switch = match parent {
                        Some(p) if p.role == HardwareRole::SwitchPort => p.address.as_str(),
                        _ => node.address.as_str(),
                    };
                    if !switches.contains(&switch) {
                        switches.push(switch);
                    }
                }
                if switches.len() <= 1 {
                    PathClass::Pix
                } else {
                    PathClass::Pxb
                }
            };
            let bottleneck = up
                .iter()
                .chain(down)
                .filter_map(|n| n.link?.current)
                .min_by(|x, y| x.bandwidth().total_cmp(&y.bandwidth()));
            result.push(HardwarePath {
                id: format!("{}/{}", a.address, b.address),
                a: a.address.clone(),
                b: b.address.clone(),
                class,
                via,
                bottleneck,
            });
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::super::pci::{fixture, read_topology};
    use super::*;

    fn class_of(
        paths: &[HardwarePath],
        a: &str,
        b: &str,
    ) -> (PathClass, Vec<String>, Option<PcieLink>) {
        let path = paths
            .iter()
            .find(|p| (p.a == a && p.b == b) || (p.a == b && p.b == a))
            .unwrap();
        (path.class, path.via.clone(), path.bottleneck)
    }

    #[test]
    fn classes_match_nvidia_topology_and_find_the_bottleneck() {
        let f = fixture::workstation("paths");
        let nodes = read_topology(&f.sys());
        let found = paths(&nodes, 1);
        // This machine: GPU and HCA under different host bridges of one NUMA node.
        let (class, via, bottleneck) = class_of(&found, "0000:01:00.0", "0000:85:00.0");
        assert_eq!(class, PathClass::Node);
        assert_eq!(
            via,
            ["0000:00:01.1", "pci0000:00", "pci0000:80", "0000:80:03.4"]
        );
        assert_eq!(
            bottleneck,
            Some(PcieLink {
                speed_gts: 5.0,
                width: 8
            })
        );
        assert_eq!(
            paths(&nodes, 2)[0].class,
            PathClass::Sys,
            "unknown NUMA on a multi-node host"
        );

        let f = fixture::switched("paths-switched");
        let nodes = read_topology(&f.sys());
        let found = paths(&nodes, 2);
        // Two GPUs behind one switch.
        let (class, via, _) = class_of(&found, "0000:03:00.0", "0000:04:00.0");
        assert_eq!(class, PathClass::Pix);
        assert_eq!(via, ["0000:02:00.0", "0000:01:00.0", "0000:02:01.0"]);
        assert_eq!(
            class_of(&found, "0000:03:00.0", "0000:05:00.0").0,
            PathClass::Pix
        );
        // Different NUMA nodes.
        assert_eq!(
            class_of(&found, "0000:03:00.0", "0000:83:00.0").0,
            PathClass::Sys
        );
        assert_eq!(found.len(), 15);
    }
}
