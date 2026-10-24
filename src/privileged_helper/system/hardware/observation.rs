//! One hardware observation for a system snapshot.

use super::devices::{HardwareDevice, device};
use super::handles::DeviceHandleCollector;
use super::paths::{HardwarePath, paths};
use super::pci::{online_numa_nodes, read_topology};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};

/// Where sysfs and procfs are mounted; tests use temporary trees.
#[derive(Clone)]
pub(in crate::privileged_helper::system) struct HardwareSources {
    pub(in crate::privileged_helper::system) sys: PathBuf,
    pub(in crate::privileged_helper::system) proc: PathBuf,
}
impl Default for HardwareSources {
    fn default() -> Self {
        Self {
            sys: "/sys".into(),
            proc: "/proc".into(),
        }
    }
}

#[derive(Default)]
pub(in crate::privileged_helper::system) struct HardwareObservation {
    pub(in crate::privileged_helper::system) devices: Vec<HardwareDevice>,
    pub(in crate::privileged_helper::system) paths: Vec<HardwarePath>,
}

impl HardwareObservation {
    /// Reads GPUs, RDMA NICs, accelerators and NVSwitches with their bridges and paths.
    pub(in crate::privileged_helper::system) fn observe(sources: &HardwareSources) -> Self {
        let nodes = read_topology(&sources.sys);
        Self {
            paths: paths(&nodes, online_numa_nodes(&sources.sys)),
            devices: nodes.iter().map(|n| device(n, &sources.proc)).collect(),
        }
    }

    /// A collector for descriptors of this observation's devices.
    pub(in crate::privileged_helper::system) fn handles(
        &self,
        sources: &HardwareSources,
    ) -> DeviceHandleCollector {
        let addresses: HashSet<_> = self.devices.iter().map(|d| d.address.clone()).collect();
        let minors: HashMap<_, _> = self
            .devices
            .iter()
            .filter_map(|d| Some((d.gpu.as_ref()?.minor?, d.address.clone())))
            .collect();
        DeviceHandleCollector::new(&sources.sys, &sources.proc, addresses, minors)
    }
}

#[cfg(test)]
pub(in crate::privileged_helper::system) fn observe_at(
    root: &std::path::Path,
) -> HardwareObservation {
    HardwareObservation::observe(&HardwareSources {
        sys: root.join("sys"),
        proc: root.join("proc"),
    })
}

#[cfg(test)]
mod tests {
    use super::super::pci::fixture;
    use super::*;

    #[test]
    fn devices_carry_gpu_and_rdma_identity_as_json() {
        let f = fixture::workstation("observation");
        let observation = observe_at(&f.root);
        let json = serde_json::to_value(&observation.devices).unwrap();
        let gpu = json
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["address"] == "0000:01:00.0")
            .unwrap();
        assert_eq!(gpu["role"], "endpoint");
        assert_eq!(gpu["kind"], "gpu");
        assert_eq!(gpu["label"], "NVIDIA GeForce RTX 5070 Ti");
        assert_eq!(gpu["class"], "0x030000");
        assert_eq!(gpu["vendor_id"], "0x10de");
        assert_eq!(gpu["gpu"]["minor"], 0);
        assert_eq!(
            gpu["gpu"]["drm"],
            serde_json::json!(["card1", "renderD128"])
        );
        assert_eq!(
            gpu["link"],
            serde_json::json!({"current": {"speed_gts": 8.0, "width": 16}, "max": {"speed_gts": 32.0, "width": 16}})
        );
        let nic = json
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["address"] == "0000:85:00.0")
            .unwrap();
        assert_eq!(nic["kind"], "rdma");
        assert_eq!(nic["label"], "ibp133s0");
        assert_eq!(nic["rdma"][0]["hca_type"], "MT26428");
        assert_eq!(nic["netdevs"], serde_json::json!(["ibp133s0"]));
        let host = json
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["address"] == "pci0000:80")
            .unwrap();
        assert_eq!(
            (host["role"].clone(), host["label"].clone()),
            (
                serde_json::json!("host_bridge"),
                serde_json::json!("Host bridge 0000:80")
            )
        );
        assert_eq!(
            serde_json::to_value(&observation.paths).unwrap()[0]["class"],
            "node"
        );
    }
}
