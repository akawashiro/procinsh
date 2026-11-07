//! One RDMA observation for a system snapshot.
//!
//! # Interface
//! - [`RdmaObservation`] (`pub(in crate::privileged_helper::system)`): `struct RdmaObservation`.
//! - [`collect_rdma_observation`] (`pub(in crate::privileged_helper::system)`): `fn collect_rdma_observation( sys: &Path, processes: &HashMap<i32, ProcessId>, deadline: Instant, ) -> RdmaObservation`.
//!
//! Supporting types: [`HashMap`], [`Instant`], [`Path`], [`ProcessId`], [`RdmaObservation`].

use super::super::ipoib;
use super::{nldev, ports, relations};
use crate::ipc::rdma::IbGid;
use crate::ipc::rdma::{RdmaPortInfo, RdmaRelation};
use crate::ipc::schema::ProcessId;
use std::{collections::HashMap, path::Path, time::Instant};

#[derive(Default)]
pub(in crate::privileged_helper::system) struct RdmaObservation {
    pub(in crate::privileged_helper::system) ports: Vec<RdmaPortInfo>,
    pub(in crate::privileged_helper::system) relations: Vec<RdmaRelation>,
    pub(in crate::privileged_helper::system) warnings: Vec<String>,
}

/// Observes RDMA resources visible in the collector's network and PID namespaces,
/// with devices and ports read under the sysfs root `sys`. Failures become
/// warnings; an IPoIB neighbor failure only omits remote GIDs.
pub(in crate::privileged_helper::system) fn collect_rdma_observation(
    sys: &Path,
    processes: &HashMap<i32, ProcessId>,
    deadline: Instant,
) -> RdmaObservation {
    let root = &sys.join("class/infiniband");
    let mut result = RdmaObservation::default();
    // Opening NETLINK_RDMA can autoload ib_core; a host without devices is not queried.
    if ports::list_rdma_devices(root).is_empty() {
        return result;
    }
    result.ports = ports::read_rdma_ports(root);
    let resources = match nldev::read_resources(deadline) {
        Ok(resources) => resources,
        Err(error) => {
            result.warnings.push(format!("RDMA: {error:#}"));
            return result;
        }
    };
    for error in &resources.errors {
        result.warnings.push(format!("RDMA: {error}"));
    }
    if resources.truncated {
        result.warnings.push(format!(
            "RDMA queue pair limit of {} reached. Some queue pairs are missing.",
            nldev::QUEUE_PAIR_LIMIT
        ));
    }
    let mut neighbors = HashMap::new();
    if resources
        .cm_ids
        .iter()
        .any(|cm| cm.destination.is_some_and(|d| d.ip().is_some()))
    {
        match ipoib::ipoib_gids(deadline) {
            Ok(found) => {
                neighbors = found
                    .into_iter()
                    .filter_map(|(ip, gid)| Some((ip, IbGid::parse(&gid)?)))
                    .collect()
            }
            Err(error) => result
                .warnings
                .push(format!("RDMA remote GIDs unavailable: {error:#}")),
        }
    }
    result.relations =
        relations::build_rdma_relations(&resources, &result.ports, &neighbors, processes);
    result
}
