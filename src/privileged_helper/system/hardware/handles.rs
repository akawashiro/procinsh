//! Processes holding character devices of the hardware view, such as
//! `/dev/nvidia0`, `/dev/dri/renderD128` or `/dev/infiniband/uverbs0`.

use super::pci::text;
use crate::privileged_helper::process::ProcessId;
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::privileged_helper::system) enum DeviceHandleKind {
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

/// One process's descriptors for one device and kind, coalesced.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(in crate::privileged_helper::system) struct DeviceHandle {
    pub(in crate::privileged_helper::system) id: String,
    pub(in crate::privileged_helper::system) process_id: ProcessId,
    pub(in crate::privileged_helper::system) device: String,
    pub(in crate::privileged_helper::system) kind: DeviceHandleKind,
    /// The lowest descriptor number.
    pub(in crate::privileged_helper::system) fd: u32,
    pub(in crate::privileged_helper::system) fd_count: usize,
}

/// Resolves character device numbers once per snapshot and coalesces descriptors.
pub(in crate::privileged_helper::system) struct DeviceHandleCollector {
    sys: PathBuf,
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

impl DeviceHandleCollector {
    /// `devices` are the hardware view's PCI addresses; `gpu_minors` maps NVIDIA
    /// device minors to them.
    pub(in crate::privileged_helper::system) fn new(
        sys: &Path,
        proc: &Path,
        devices: HashSet<String>,
        gpu_minors: HashMap<u32, String>,
    ) -> Self {
        let nvidia_major = text(&proc.join("devices")).and_then(|devices| {
            devices.lines().find_map(|line| {
                let (major, name) = line.trim().split_once(' ')?;
                (name == "nvidia" || name == "nvidia-frontend").then(|| major.parse().ok())?
            })
        });
        Self {
            sys: sys.to_owned(),
            devices,
            nvidia_major,
            nvidia_minors: gpu_minors,
            resolved: HashMap::new(),
            handles: HashMap::new(),
        }
    }

    fn resolve(&self, rdev: u64) -> Option<(DeviceHandleKind, String)> {
        let (major, minor) = (libc::major(rdev), libc::minor(rdev));
        if Some(major) == self.nvidia_major {
            // /dev/nvidiactl and other control minors have no device.
            return Some((
                DeviceHandleKind::Nvidia,
                self.nvidia_minors.get(&minor)?.clone(),
            ));
        }
        let path = fs::canonicalize(self.sys.join(format!("dev/char/{major}:{minor}"))).ok()?;
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
    pub(in crate::privileged_helper::system) fn observe(
        &mut self,
        process: ProcessId,
        fd: u32,
        rdev: u64,
    ) {
        if !self.resolved.contains_key(&rdev) {
            let resolved = self.resolve(rdev).filter(|(_, d)| self.devices.contains(d));
            self.resolved.insert(rdev, resolved);
        }
        let Some((kind, device)) = self.resolved[&rdev].clone() else {
            return;
        };
        let entry = self
            .handles
            .entry((process, kind, device))
            .or_insert((fd, 0));
        entry.0 = entry.0.min(fd);
        entry.1 += 1;
    }

    /// Handles of `live` processes, ordered by process and device.
    pub(in crate::privileged_helper::system) fn finish(
        self,
        live: &HashSet<ProcessId>,
    ) -> Vec<DeviceHandle> {
        let mut handles: Vec<_> = self
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
}

#[cfg(test)]
mod tests {
    use super::super::pci::fixture;
    use super::*;

    #[test]
    fn character_devices_resolve_to_pci_functions() {
        let f = fixture::workstation("handles");
        let gpu = f.sys().join("devices/pci0000:00/0000:00:01.1/0000:01:00.0");
        f.char_device("226:128", &gpu.join("drm/renderD128"));
        f.char_device(
            "231:192",
            &f.sys()
                .join("devices/pci0000:80/0000:80:03.4/0000:85:00.0/infiniband_verbs/uverbs0"),
        );
        f.char_device("1:3", &f.sys().join("devices/virtual/mem/null"));
        f.write(
            "proc/devices",
            "Character devices:\n  1 mem\n195 nvidia\n195 nvidiactl\n226 drm",
        );
        let devices = HashSet::from(["0000:01:00.0".to_owned(), "0000:85:00.0".to_owned()]);
        let mut collector = DeviceHandleCollector::new(
            &f.sys(),
            &f.proc(),
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
            collector.observe(process, fd, rdev);
        }
        collector.observe(other, 3, libc::makedev(226, 128));
        let handles = collector.finish(&HashSet::from([process]));
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
