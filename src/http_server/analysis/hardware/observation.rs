//! One hardware observation for a system snapshot.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`HardwareSources`] | `pub` | `struct HardwareSources` |
//! | [`HardwareObservation`] | `pub` | `struct HardwareObservation` |
//! | [`acquired_hardware_sources`] | `pub` | `fn acquired_hardware_sources(files: `[`crate::ipc::hardware::HardwareFiles`]`) -> `[`HardwareSources`] |
//! | [`text_hardware_sources`] | `pub(super)` | `fn text_hardware_sources( state: &`[`HardwareSources`]`, path: &`[`std::path::Path`]`, ) -> `[`Option`]`<`[`String`]`>` |
//! | [`names_hardware_sources`] | `pub(super)` | `fn names_hardware_sources( state: &`[`HardwareSources`]`, path: &`[`std::path::Path`]`, ) -> `[`Vec`]`<`[`String`]`>` |
//! | [`link_hardware_sources`] | `pub(super)` | `fn link_hardware_sources( state: &`[`HardwareSources`]`, path: &`[`std::path::Path`]`, ) -> `[`Option`]`<`[`PathBuf`]`>` |
//! | [`hardware_sources_is_dir`] | `pub(super)` | `fn hardware_sources_is_dir(state: &`[`HardwareSources`]`, path: &`[`std::path::Path`]`) -> bool` |
//! | [`observe_hardware_observation`] | `pub` | `fn observe_hardware_observation(sources: &`[`HardwareSources`]`) -> `[`HardwareObservation`] |
//! | [`handles_hardware_observation`] | `pub` | `fn handles_hardware_observation( state: &`[`HardwareObservation`]`, sources: &`[`HardwareSources`]`, ) -> `[`DeviceHandleCollector`] |

use super::handles::DeviceHandleCollector;
use super::pci::{online_numa_nodes, read_topology};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};
use {super::devices::build_hardware_device, crate::ipc::hardware::HardwareDevice};
use {super::paths::build_hardware_paths, crate::ipc::hardware::HardwarePath};

/// Logical roots and an in-memory view of helper-acquired attributes.
#[derive(Clone)]
pub struct HardwareSources {
    pub sys: PathBuf,
    pub proc: PathBuf,
    texts: HashMap<PathBuf, String>,
    directories: HashMap<PathBuf, Vec<String>>,
    links: HashMap<PathBuf, PathBuf>,
}
impl Default for HardwareSources {
    fn default() -> Self {
        acquired_hardware_sources(crate::ipc::hardware::HardwareFiles::default())
    }
}
pub fn acquired_hardware_sources(files: crate::ipc::hardware::HardwareFiles) -> HardwareSources {
    at_hardware_sources("/sys".into(), "/proc".into(), files)
}

fn at_hardware_sources(
    sys: PathBuf,
    proc: PathBuf,
    files: crate::ipc::hardware::HardwareFiles,
) -> HardwareSources {
    HardwareSources {
        sys,
        proc,
        texts: files
            .texts
            .into_iter()
            .map(|(p, v)| (p.into(), v))
            .collect(),
        directories: files
            .directories
            .into_iter()
            .map(|(p, v)| (p.into(), v))
            .collect(),
        links: files
            .links
            .into_iter()
            .map(|(p, v)| (p.into(), v.into()))
            .collect(),
    }
}

pub(super) fn text_hardware_sources(
    state: &HardwareSources,
    path: &std::path::Path,
) -> Option<String> {
    let value = state.texts.get(path)?.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

pub(super) fn names_hardware_sources(
    state: &HardwareSources,
    path: &std::path::Path,
) -> Vec<String> {
    state.directories.get(path).cloned().unwrap_or_default()
}

pub(super) fn link_hardware_sources(
    state: &HardwareSources,
    path: &std::path::Path,
) -> Option<PathBuf> {
    state.links.get(path).cloned()
}

pub(super) fn hardware_sources_is_dir(state: &HardwareSources, path: &std::path::Path) -> bool {
    state.directories.contains_key(path)
}

#[cfg(test)]
pub(super) fn fixture_hardware_sources(sys: PathBuf, proc: PathBuf) -> HardwareSources {
    let files = super::acquisition::collect_hardware_files(&sys, &proc);
    at_hardware_sources(sys, proc, files)
}
#[cfg(test)]
#[path = "../../../privileged_helper/system/hardware_capture.rs"]
pub(crate) mod acquisition;

#[derive(Default)]
pub struct HardwareObservation {
    pub devices: Vec<HardwareDevice>,
    pub paths: Vec<HardwarePath>,
}

/// Reads GPUs, RDMA NICs, accelerators and NVSwitches with their bridges and paths.
pub fn observe_hardware_observation(sources: &HardwareSources) -> HardwareObservation {
    let nodes = read_topology(sources, &sources.sys);
    HardwareObservation {
        paths: build_hardware_paths(&nodes, online_numa_nodes(sources, &sources.sys)),
        devices: nodes
            .iter()
            .map(|n| build_hardware_device(sources, n, &sources.proc))
            .collect(),
    }
}

/// A collector for descriptors of this observation's devices.
pub fn handles_hardware_observation(
    state: &HardwareObservation,
    sources: &HardwareSources,
) -> DeviceHandleCollector {
    use crate::http_server::analysis::hardware::handles::create_device_handle_collector;

    let addresses: HashSet<_> = state.devices.iter().map(|d| d.address.clone()).collect();
    let minors: HashMap<_, _> = state
        .devices
        .iter()
        .filter_map(|d| Some((d.gpu.as_ref()?.minor?, d.address.clone())))
        .collect();
    create_device_handle_collector(sources, addresses, minors)
}

#[cfg(test)]
pub fn observe_at(root: &std::path::Path) -> HardwareObservation {
    observe_hardware_observation(&fixture_hardware_sources(
        root.join("sys"),
        root.join("proc"),
    ))
}

#[cfg(test)]
mod tests {
    use super::{super::pci::fixture, *};

    #[test]
    fn captured_hardware_survives_removal_of_all_source_files() {
        use crate::http_server::analysis::hardware::pci::fixture::{
            proc_sysfs_fixture, sys_sysfs_fixture,
        };

        let f = fixture::workstation("captured-only");
        let sys = sys_sysfs_fixture(&f);
        let proc = proc_sysfs_fixture(&f);
        let files = super::super::acquisition::collect_hardware_files(&sys, &proc);
        assert!(!files.truncated);
        let encoded = crate::ipc::codec::encode(&files, 16 * 1024 * 1024).unwrap();
        let captured = crate::ipc::codec::decode(&encoded).unwrap();
        std::fs::remove_dir_all(&f.root).unwrap();
        let sources = super::at_hardware_sources(sys, proc, captured);
        let observation = super::observe_hardware_observation(&sources);
        let gpu = observation
            .devices
            .iter()
            .find(|d| d.address == "0000:01:00.0")
            .unwrap();
        assert_eq!(
            gpu.gpu.as_ref().unwrap().model.as_deref(),
            Some("NVIDIA GeForce RTX 5070 Ti")
        );
        assert!(!observation.paths.is_empty());
    }

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
