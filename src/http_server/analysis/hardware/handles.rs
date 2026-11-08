//! Processes holding character devices of the hardware view, such as
//! `/dev/nvidia0`, `/dev/dri/renderD128` or `/dev/infiniband/uverbs0`.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`DeviceHandleCollector`] | `pub` | `struct DeviceHandleCollector` |
//! | [`create_device_handle_collector`] | `pub` | `fn create_device_handle_collector( sources: &`[`HardwareSources`]`, devices: `[`HashSet`]`<`[`String`]`>, gpu_minors: `[`HashMap`]`<u32, `[`String`]`>, ) -> `[`DeviceHandleCollector`] |
//! | [`observe_device_handle_collector`] | `pub` | `fn observe_device_handle_collector( state: &mut `[`DeviceHandleCollector`]`, process: `[`ProcessId`]`, fd: u32, rdev: u64, )` |
//! | [`finish_device_handle_collector`] | `pub` | `fn finish_device_handle_collector( state: `[`DeviceHandleCollector`]`, live: &`[`HashSet`]`<`[`ProcessId`]`>, ) -> `[`Vec`]`<`[`DeviceHandle`]`>` |

use super::observation::HardwareSources;
use crate::ipc::hardware::{DeviceHandle, DeviceHandleKind};
use crate::ipc::schema::ProcessId;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::hardware::DeviceHandleKind")]
#[serde(rename_all = "snake_case")]
enum DeviceHandleKindJson {
    /// `/dev/nvidiaN`.
    Nvidia,
    DrmRender,
    DrmCard,
    /// `/dev/infiniband/uverbsN`.
    Uverbs,
    /// `/dev/infiniband/umadN`.
    Umad,
    /// `/dev/accel/accelN`.
    Accel,
}
impl serde::Serialize for crate::ipc::hardware::DeviceHandleKind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        DeviceHandleKindJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::hardware::DeviceHandleKind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        DeviceHandleKindJson::deserialize(d)
    }
}

/// One process's descriptors for one device and kind, coalesced.
#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::hardware::DeviceHandle")]
struct DeviceHandleJson {
    pub id: String,
    pub process_id: ProcessId,
    pub device: String,
    pub kind: DeviceHandleKind,
    /// The lowest descriptor number.
    pub fd: u32,
    pub fd_count: usize,
}
impl serde::Serialize for crate::ipc::hardware::DeviceHandle {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        DeviceHandleJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::hardware::DeviceHandle {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        DeviceHandleJson::deserialize(d)
    }
}

/// Resolves character device numbers once per snapshot and coalesces descriptors.
pub struct DeviceHandleCollector {
    sources: HardwareSources,
    devices: HashSet<String>,
    nvidia_major: Option<u32>,
    nvidia_minors: HashMap<u32, String>,
    resolved: HashMap<u64, Option<(DeviceHandleKind, String)>>,
    handles: HashMap<(ProcessId, DeviceHandleKind, String), (u32, usize)>,
}

/// The innermost PCI function in a sysfs device path.
fn function_address(path: &Path) -> Option<String> {
    path.components().rev().find_map(|c| {
        let part = c.as_os_str().to_str()?;
        (part.len() >= 12 && part.as_bytes()[part.len() - 2] == b'.').then(|| part.to_owned())
    })
}

/// `devices` are the hardware view's PCI addresses; `gpu_minors` maps NVIDIA
/// device minors to them.
pub fn create_device_handle_collector(
    sources: &HardwareSources,
    devices: HashSet<String>,
    gpu_minors: HashMap<u32, String>,
) -> DeviceHandleCollector {
    use crate::http_server::analysis::hardware::observation::text_hardware_sources;

    let nvidia_major =
        text_hardware_sources(sources, &sources.proc.join("devices")).and_then(|devices| {
            devices.lines().find_map(|line| {
                let (major, name) = line.trim().split_once(' ')?;
                (name == "nvidia" || name == "nvidia-frontend").then(|| major.parse().ok())?
            })
        });
    DeviceHandleCollector {
        sources: sources.clone(),
        devices,
        nvidia_major,
        nvidia_minors: gpu_minors,
        resolved: HashMap::new(),
        handles: HashMap::new(),
    }
}

fn resolve_device_handle_collector(
    state: &DeviceHandleCollector,
    rdev: u64,
) -> Option<(DeviceHandleKind, String)> {
    use crate::http_server::analysis::hardware::observation::link_hardware_sources;

    let (major, minor) = (libc::major(rdev), libc::minor(rdev));
    if Some(major) == state.nvidia_major {
        // /dev/nvidiactl and other control minors have no device.
        return Some((
            DeviceHandleKind::Nvidia,
            state.nvidia_minors.get(&minor)?.clone(),
        ));
    }
    let path = link_hardware_sources(
        &state.sources,
        &state.sources.sys.join(format!("dev/char/{major}:{minor}")),
    )?;
    let text = path.to_string_lossy();
    let kind = if text.contains("/drm/renderD") {
        DeviceHandleKind::DrmRender
    } else if text.contains("/drm/card") {
        DeviceHandleKind::DrmCard
    } else if text.contains("/infiniband_verbs/") {
        DeviceHandleKind::Uverbs
    } else if text.contains("/infiniband_mad/") {
        DeviceHandleKind::Umad
    } else if text.contains("/accel/") {
        DeviceHandleKind::Accel
    } else {
        return None;
    };
    Some((kind, function_address(&path)?))
}

/// Records descriptor `fd` of `process` for character device `rdev`.
pub fn observe_device_handle_collector(
    state: &mut DeviceHandleCollector,
    process: ProcessId,
    fd: u32,
    rdev: u64,
) {
    if !state.resolved.contains_key(&rdev) {
        let resolved =
            resolve_device_handle_collector(state, rdev).filter(|(_, d)| state.devices.contains(d));
        state.resolved.insert(rdev, resolved);
    }
    let Some((kind, device)) = state.resolved[&rdev].clone() else {
        return;
    };
    let entry = state
        .handles
        .entry((process, kind, device))
        .or_insert((fd, 0));
    entry.0 = entry.0.min(fd);
    entry.1 += 1;
}

/// Handles of `live` processes, ordered by process and device.
pub fn finish_device_handle_collector(
    state: DeviceHandleCollector,
    live: &HashSet<ProcessId>,
) -> Vec<DeviceHandle> {
    let mut handles: Vec<_> = state
        .handles
        .into_iter()
        .filter(|((process, _, _), _)| live.contains(process))
        .map(
            |((process_id, kind, device), (fd, fd_count))| DeviceHandle {
                id: format!(
                    "{}:{}:{}:{}",
                    process_id.pid,
                    process_id.start_time_ticks,
                    serde_json::to_value(kind).unwrap().as_str().unwrap(),
                    device
                ),
                process_id,
                device,
                kind,
                fd,
                fd_count,
            },
        )
        .collect();
    handles.sort_by(|a, b| a.id.cmp(&b.id));
    handles
}

#[cfg(test)]
mod tests {
    use {
        super::super::pci::fixture,
        crate::ipc::{hardware::DeviceHandleKind, schema::ProcessId},
        std::collections::{HashMap, HashSet},
    };

    #[test]
    fn character_devices_resolve_to_pci_functions() {
        use crate::http_server::analysis::hardware::pci::fixture::{
            char_device_sysfs_fixture, sources_sysfs_fixture, sys_sysfs_fixture,
            write_sysfs_fixture,
        };

        let f = fixture::workstation("handles");
        let gpu = sys_sysfs_fixture(&f).join("devices/pci0000:00/0000:00:01.1/0000:01:00.0");
        char_device_sysfs_fixture(&f, "226:128", &gpu.join("drm/renderD128"));
        char_device_sysfs_fixture(
            &f,
            "231:192",
            &sys_sysfs_fixture(&f)
                .join("devices/pci0000:80/0000:80:03.4/0000:85:00.0/infiniband_verbs/uverbs0"),
        );
        char_device_sysfs_fixture(
            &f,
            "1:3",
            &sys_sysfs_fixture(&f).join("devices/virtual/mem/null"),
        );
        write_sysfs_fixture(
            &f,
            "proc/devices",
            "Character devices:\n  1 mem\n195 nvidia\n195 nvidiactl\n226 drm",
        );
        let devices = HashSet::from(["0000:01:00.0".to_owned(), "0000:85:00.0".to_owned()]);
        let mut collector = super::create_device_handle_collector(
            &sources_sysfs_fixture(&f),
            devices,
            HashMap::from([(0, "0000:01:00.0".to_owned())]),
        );
        let process = ProcessId {
            pid: 7,
            start_time_ticks: 9,
        };
        let other = ProcessId {
            pid: 8,
            start_time_ticks: 9,
        };
        for (fd, rdev) in [
            (19, libc::makedev(195, 0)),
            (12, libc::makedev(195, 0)),
            (13, libc::makedev(195, 255)), // nvidiactl
            (20, libc::makedev(226, 128)),
            (21, libc::makedev(231, 192)),
            (0, libc::makedev(1, 3)),
        ] {
            super::observe_device_handle_collector(&mut collector, process, fd, rdev);
        }
        super::observe_device_handle_collector(&mut collector, other, 3, libc::makedev(226, 128));
        let handles = super::finish_device_handle_collector(collector, &HashSet::from([process]));
        let summary: Vec<_> = handles
            .iter()
            .map(|h| (h.id.as_str(), h.kind, h.fd, h.fd_count))
            .collect();
        assert_eq!(
            summary,
            [
                (
                    "7:9:drm_render:0000:01:00.0",
                    DeviceHandleKind::DrmRender,
                    20,
                    1
                ),
                ("7:9:nvidia:0000:01:00.0", DeviceHandleKind::Nvidia, 12, 2),
                ("7:9:uverbs:0000:85:00.0", DeviceHandleKind::Uverbs, 21, 1),
            ]
        );
    }
}
