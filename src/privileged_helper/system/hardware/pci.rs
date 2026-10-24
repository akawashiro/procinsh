//! PCI devices of interest and their upstream bridges from sysfs.
//!
//! Only plain sysfs attributes are read. `config` and BAR resize files are never
//! opened, since they can wake a runtime-suspended device.

use serde::Serialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::privileged_helper::system) enum HardwareRole {
    HostBridge,
    RootPort,
    SwitchPort,
    Endpoint,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::privileged_helper::system) enum HardwareKind {
    Gpu,
    /// An RDMA-capable NIC (InfiniBand or RoCE).
    Rdma,
    /// A processing accelerator (PCI class 0x12).
    Accelerator,
    NvSwitch,
}

/// The negotiated or maximum link of a device to its upstream port.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub(in crate::privileged_helper::system) struct PcieLink {
    pub(in crate::privileged_helper::system) speed_gts: f64,
    pub(in crate::privileged_helper::system) width: u32,
}
impl PcieLink {
    /// Usable bandwidth in GB/s after line encoding (8b/10b up to 5 GT/s, then 128b/130b).
    pub(in crate::privileged_helper::system) fn bandwidth(self) -> f64 {
        let encoding = if self.speed_gts <= 5.0 {
            0.8
        } else {
            128.0 / 130.0
        };
        self.speed_gts * encoding * f64::from(self.width) / 8.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub(in crate::privileged_helper::system) struct PcieLinkInfo {
    pub(in crate::privileged_helper::system) current: Option<PcieLink>,
    pub(in crate::privileged_helper::system) max: Option<PcieLink>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub(in crate::privileged_helper::system) struct BarInfo {
    pub(in crate::privileged_helper::system) index: u32,
    pub(in crate::privileged_helper::system) bytes: u64,
    pub(in crate::privileged_helper::system) prefetchable: bool,
}

/// A PCI node: a host bridge, a bridge on the way to a device of interest, or the device.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::privileged_helper::system) struct PciNode {
    pub(in crate::privileged_helper::system) address: String,
    pub(in crate::privileged_helper::system) parent: Option<String>,
    pub(in crate::privileged_helper::system) role: HardwareRole,
    pub(in crate::privileged_helper::system) kind: Option<HardwareKind>,
    pub(in crate::privileged_helper::system) path: PathBuf,
    pub(in crate::privileged_helper::system) class: Option<u32>,
    pub(in crate::privileged_helper::system) vendor_id: Option<u32>,
    pub(in crate::privileged_helper::system) device_id: Option<u32>,
    pub(in crate::privileged_helper::system) driver: Option<String>,
    pub(in crate::privileged_helper::system) numa_node: Option<u32>,
    pub(in crate::privileged_helper::system) link: Option<PcieLinkInfo>,
    pub(in crate::privileged_helper::system) power_state: Option<String>,
    pub(in crate::privileged_helper::system) runtime_status: Option<String>,
    pub(in crate::privileged_helper::system) iommu_group: Option<u32>,
    pub(in crate::privileged_helper::system) bars: Vec<BarInfo>,
}

pub(super) fn text(path: &Path) -> Option<String> {
    let value = fs::read_to_string(path).ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn hex(path: &Path) -> Option<u32> {
    u32::from_str_radix(text(path)?.trim_start_matches("0x"), 16).ok()
}

pub(super) fn names(path: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn is_function_address(name: &str) -> bool {
    let bytes = name.as_bytes();
    // dddd:bb:dd.f with hexadecimal digits; the domain can be wider than four digits.
    name.len() >= 12
        && bytes[name.len() - 2] == b'.'
        && bytes[name.len() - 5] == b':'
        && bytes[name.len() - 8] == b':'
        && name
            .bytes()
            .all(|b| b.is_ascii_hexdigit() || b == b':' || b == b'.')
}

fn is_host_bridge(name: &str) -> bool {
    name.strip_prefix("pci").is_some_and(|rest| {
        rest.contains(':') && rest.bytes().all(|b| b.is_ascii_hexdigit() || b == b':')
    })
}

fn link_speed(path: &Path) -> Option<f64> {
    text(path)?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
        .filter(|s: &f64| *s > 0.0)
}

fn link(dir: &Path, prefix: &str) -> Option<PcieLink> {
    let width: u32 = text(&dir.join(format!("{prefix}_link_width")))?
        .parse()
        .ok()?;
    Some(PcieLink {
        speed_gts: link_speed(&dir.join(format!("{prefix}_link_speed")))?,
        width: (width > 0).then_some(width)?,
    })
}

fn bars(dir: &Path) -> Vec<BarInfo> {
    const IORESOURCE_MEM: u64 = 0x200;
    const IORESOURCE_PREFETCH: u64 = 0x2000;
    text(&dir.join("resource"))
        .unwrap_or_default()
        .lines()
        .take(6)
        .enumerate()
        .filter_map(|(index, line)| {
            let fields: Vec<u64> = line
                .split_whitespace()
                .map(|f| u64::from_str_radix(f.trim_start_matches("0x"), 16).ok())
                .collect::<Option<_>>()?;
            let [start, end, flags] = fields[..] else {
                return None;
            };
            (end > start && flags & IORESOURCE_MEM != 0).then_some(BarInfo {
                index: index as u32,
                bytes: end - start + 1,
                prefetchable: flags & IORESOURCE_PREFETCH != 0,
            })
        })
        .collect()
}

/// The kind of a device worth showing; BMC display adapters are not GPUs.
fn kind(dir: &Path, class: u32, vendor: Option<u32>) -> Option<HardwareKind> {
    const ASPEED: u32 = 0x1a03;
    const MATROX: u32 = 0x102b;
    const NVIDIA: u32 = 0x10de;
    if dir.join("infiniband").is_dir() || class >> 8 == 0x0c06 {
        return Some(HardwareKind::Rdma);
    }
    match class >> 8 {
        0x0300 | 0x0302 if !matches!(vendor, Some(ASPEED | MATROX)) => Some(HardwareKind::Gpu),
        0x1200..=0x12ff => Some(HardwareKind::Accelerator),
        0x0680 if vendor == Some(NVIDIA) => Some(HardwareKind::NvSwitch),
        _ => None,
    }
}

fn node(
    address: &str,
    parent: Option<String>,
    dir: PathBuf,
    kind: Option<HardwareKind>,
) -> PciNode {
    let host = is_host_bridge(address);
    let runtime_status = text(&dir.join("power/runtime_status"));
    // Link registers of a suspended device are not read; they would read as invalid.
    let awake = runtime_status.as_deref() != Some("suspended");
    PciNode {
        address: address.into(),
        role: match (host, kind, &parent) {
            (true, _, _) => HardwareRole::HostBridge,
            (_, Some(_), _) => HardwareRole::Endpoint,
            (_, None, Some(parent)) if is_host_bridge(parent) => HardwareRole::RootPort,
            _ => HardwareRole::SwitchPort,
        },
        parent,
        kind,
        class: (!host).then(|| hex(&dir.join("class"))).flatten(),
        vendor_id: (!host).then(|| hex(&dir.join("vendor"))).flatten(),
        device_id: (!host).then(|| hex(&dir.join("device"))).flatten(),
        driver: fs::read_link(dir.join("driver"))
            .ok()
            .and_then(|p| Some(p.file_name()?.to_string_lossy().into_owned())),
        numa_node: text(&dir.join("numa_node")).and_then(|n| n.parse().ok()),
        link: (!host && awake).then(|| PcieLinkInfo {
            current: link(&dir, "current"),
            max: link(&dir, "max"),
        }),
        power_state: text(&dir.join("power_state")),
        runtime_status,
        iommu_group: fs::read_link(dir.join("iommu_group"))
            .ok()
            .and_then(|p| p.file_name()?.to_str()?.parse().ok()),
        bars: if kind.is_some() { bars(&dir) } else { vec![] },
        path: dir,
    }
}

/// Devices of interest and every bridge above them, parents before children.
pub(in crate::privileged_helper::system) fn read_topology(sys: &Path) -> Vec<PciNode> {
    let mut chains = Vec::new();
    for name in names(&sys.join("bus/pci/devices")) {
        let Ok(dir) = fs::canonicalize(sys.join("bus/pci/devices").join(&name)) else {
            continue;
        };
        let class = hex(&dir.join("class")).unwrap_or_default();
        let vendor = hex(&dir.join("vendor"));
        let Some(kind) = kind(&dir, class, vendor) else {
            continue;
        };
        // The chain from the host bridge: pci0000:80 / 0000:80:03.4 / 0000:85:00.0.
        let mut chain = Vec::new();
        let mut base = PathBuf::new();
        for component in dir.components() {
            base.push(component);
            let part = component.as_os_str().to_string_lossy();
            if is_host_bridge(&part) {
                chain.clear();
                chain.push((part.into_owned(), base.clone()));
            } else if !chain.is_empty() && is_function_address(&part) {
                chain.push((part.into_owned(), base.clone()));
            }
        }
        if chain.last().is_some_and(|(address, _)| *address == name) {
            chains.push((chain, kind));
        }
    }
    let mut nodes: BTreeMap<String, PciNode> = BTreeMap::new();
    let mut order = Vec::new();
    for (chain, kind) in chains {
        for (index, (address, dir)) in chain.iter().enumerate() {
            let endpoint = index + 1 == chain.len();
            if nodes
                .get(address)
                .is_some_and(|n| !endpoint || n.kind.is_some())
            {
                continue;
            }
            let parent = index.checked_sub(1).map(|i| chain[i].0.clone());
            if !nodes.contains_key(address) {
                order.push(address.clone());
            }
            nodes.insert(
                address.clone(),
                node(address, parent, dir.clone(), endpoint.then_some(kind)),
            );
        }
    }
    let mut result: Vec<_> = order.into_iter().filter_map(|a| nodes.remove(&a)).collect();
    // Parents first: sort by chain depth, then address.
    let depth = |node: &PciNode, all: &[PciNode]| {
        let mut depth = 0;
        let mut parent = node.parent.clone();
        while let Some(p) = parent {
            depth += 1;
            parent = all
                .iter()
                .find(|n| n.address == p)
                .and_then(|n| n.parent.clone());
        }
        depth
    };
    let depths: Vec<_> = result.iter().map(|n| depth(n, &result)).collect();
    let mut indexed: Vec<_> = depths.into_iter().zip(result.drain(..)).collect();
    indexed.sort_by(|(da, a), (db, b)| da.cmp(db).then_with(|| a.address.cmp(&b.address)));
    indexed.into_iter().map(|(_, n)| n).collect()
}

/// Online NUMA node count, from `devices/system/node/online` such as `0` or `0-1`.
pub(in crate::privileged_helper::system) fn online_numa_nodes(sys: &Path) -> usize {
    text(&sys.join("devices/system/node/online"))
        .map(|ranges| {
            ranges
                .split(',')
                .filter_map(|range| {
                    let (start, end) = range.split_once('-').unwrap_or((range, range));
                    Some(end.parse::<usize>().ok()? + 1 - start.parse::<usize>().ok()?)
                })
                .sum()
        })
        .unwrap_or(1)
}

#[cfg(test)]
pub(in crate::privileged_helper::system) mod fixture {
    //! A temporary sysfs/procfs tree shaped like real kernels.
    use std::{
        fs,
        os::unix::fs::symlink,
        path::{Path, PathBuf},
    };

    pub(in crate::privileged_helper::system) struct SysfsFixture {
        pub(in crate::privileged_helper::system) root: PathBuf,
    }
    impl Drop for SysfsFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
    impl SysfsFixture {
        pub(in crate::privileged_helper::system) fn new(name: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("procinsh-sysfs-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&root);
            fs::create_dir_all(root.join("sys/bus/pci/devices")).unwrap();
            fs::create_dir_all(root.join("sys/dev/char")).unwrap();
            fs::create_dir_all(root.join("proc")).unwrap();
            Self { root }
        }
        pub(in crate::privileged_helper::system) fn sys(&self) -> PathBuf {
            self.root.join("sys")
        }
        pub(in crate::privileged_helper::system) fn proc(&self) -> PathBuf {
            self.root.join("proc")
        }
        pub(in crate::privileged_helper::system) fn write(&self, path: &str, value: &str) {
            let path = self.root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, format!("{value}\n")).unwrap();
        }
        /// Adds a device at `chain` (host bridge first) with attributes.
        pub(in crate::privileged_helper::system) fn device(
            &self,
            chain: &[&str],
            attributes: &[(&str, &str)],
        ) -> PathBuf {
            let dir = chain
                .iter()
                .fold(self.sys().join("devices"), |dir, c| dir.join(c));
            fs::create_dir_all(&dir).unwrap();
            for (name, value) in attributes {
                if let Some(target) = value.strip_prefix("->") {
                    let _ = symlink(target, dir.join(name));
                } else {
                    let file = dir.join(name);
                    fs::create_dir_all(file.parent().unwrap()).unwrap();
                    fs::write(file, format!("{value}\n")).unwrap();
                }
            }
            let name = chain.last().unwrap();
            let link = self.sys().join("bus/pci/devices").join(name);
            if !link.exists() {
                symlink(&dir, link).unwrap();
            }
            dir
        }
        pub(in crate::privileged_helper::system) fn char_device(
            &self,
            number: &str,
            target: &Path,
        ) {
            fs::create_dir_all(target).unwrap();
            symlink(target, self.sys().join("dev/char").join(number)).unwrap();
        }
    }

    /// This machine: a GPU and an InfiniBand HCA under root ports of different host
    /// bridges on one NUMA node, plus a BMC display adapter.
    pub(in crate::privileged_helper::system) fn workstation(name: &str) -> SysfsFixture {
        let f = SysfsFixture::new(name);
        let bridge = |speed: &'static str, width: &'static str| {
            vec![
                ("class", "0x060400"),
                ("current_link_speed", speed),
                ("current_link_width", width),
                ("max_link_speed", "8.0 GT/s PCIe"),
                ("max_link_width", width),
                ("numa_node", "-1"),
            ]
        };
        f.device(
            &["pci0000:00", "0000:00:01.1"],
            &bridge("8.0 GT/s PCIe", "16"),
        );
        f.device(&["pci0000:00", "0000:00:01.1", "0000:01:00.0"], &[
            ("class", "0x030000"), ("vendor", "0x10de"), ("device", "0x2c05"),
            ("current_link_speed", "8.0 GT/s PCIe"), ("current_link_width", "16"),
            ("max_link_speed", "32.0 GT/s PCIe"), ("max_link_width", "16"),
            ("numa_node", "-1"), ("power_state", "D0"), ("power/runtime_status", "active"),
            ("resource", "0x00000000f4000000 0x00000000f7ffffff 0x0000000000040200\n0x0000038090000000 0x000003809fffffff 0x000000000014220c\n0x0000000000000000 0x0000000000000000 0x0000000000000000"),
            ("drm/card1/dev", "226:1"), ("drm/renderD128/dev", "226:128"),
        ]);
        f.device(
            &["pci0000:00", "0000:00:01.1", "0000:01:00.1"],
            &[("class", "0x040300"), ("vendor", "0x10de")],
        );
        f.device(
            &["pci0000:80", "0000:80:03.4"],
            &bridge("5.0 GT/s PCIe", "8"),
        );
        f.device(
            &["pci0000:80", "0000:80:03.4", "0000:85:00.0"],
            &[
                ("class", "0x0c0600"),
                ("vendor", "0x15b3"),
                ("device", "0x673c"),
                ("current_link_speed", "5.0 GT/s PCIe"),
                ("current_link_width", "8"),
                ("max_link_speed", "5.0 GT/s PCIe"),
                ("max_link_width", "8"),
                ("numa_node", "-1"),
                ("infiniband/ibp133s0/hca_type", "MT26428"),
                ("infiniband/ibp133s0/fw_ver", "2.10.720"),
                ("net/ibp133s0/type", "32"),
            ],
        );
        // A BMC VGA adapter is not a GPU.
        f.device(
            &["pci0000:c0", "0000:c0:01.1", "0000:c1:00.0"],
            &[("class", "0x030000"), ("vendor", "0x1a03")],
        );
        f.write("sys/devices/system/node/online", "0");
        f.write(
            "proc/driver/nvidia/gpus/0000:01:00.0/information",
            "Model: \t\t NVIDIA GeForce RTX 5070 Ti\nGPU UUID: \t GPU-dffb775f\nDevice Minor: \t 0",
        );
        f
    }

    /// A DGX-like tree: two NUMA nodes, each with a PCIe switch holding two GPUs and a NIC.
    pub(in crate::privileged_helper::system) fn switched(name: &str) -> SysfsFixture {
        let f = SysfsFixture::new(name);
        for (bridge, bus, numa) in [("pci0000:00", 0u8, "0"), ("pci0000:80", 0x80, "1")] {
            let attrs = |class: &'static str, vendor: &'static str| {
                vec![
                    ("class", class),
                    ("vendor", vendor),
                    ("numa_node", numa),
                    ("current_link_speed", "16.0 GT/s PCIe"),
                    ("current_link_width", "16"),
                    ("max_link_speed", "16.0 GT/s PCIe"),
                    ("max_link_width", "16"),
                ]
            };
            // Root port, switch upstream port, three downstream ports and their devices.
            let rp = format!("0000:{bus:02x}:01.0");
            let usp = format!("0000:{:02x}:00.0", bus + 1);
            f.device(&[bridge, &rp], &attrs("0x060400", "0x1022"));
            f.device(&[bridge, &rp, &usp], &attrs("0x060400", "0x1000"));
            for port in 0..3u8 {
                let dsp = format!("0000:{:02x}:{port:02x}.0", bus + 2);
                let endpoint = format!("0000:{:02x}:00.0", bus + 3 + port);
                f.device(&[bridge, &rp, &usp, &dsp], &attrs("0x060400", "0x1000"));
                let mut device = if port < 2 {
                    attrs("0x030200", "0x10de")
                } else {
                    attrs("0x020000", "0x15b3")
                };
                if port == 2 {
                    device.push(("infiniband/mlx5_0/hca_type", "MT4123"));
                }
                f.device(&[bridge, &rp, &usp, &dsp, &endpoint], &device);
            }
        }
        f.write("sys/devices/system/node/online", "0-1");
        f
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn topology_keeps_devices_of_interest_and_their_bridges() {
        let f = fixture::workstation("pci");
        let nodes = read_topology(&f.sys());
        let summary: Vec<_> = nodes
            .iter()
            .map(|n| (n.address.as_str(), n.parent.as_deref(), n.role, n.kind))
            .collect();
        assert_eq!(
            summary,
            [
                ("pci0000:00", None, HardwareRole::HostBridge, None),
                ("pci0000:80", None, HardwareRole::HostBridge, None),
                (
                    "0000:00:01.1",
                    Some("pci0000:00"),
                    HardwareRole::RootPort,
                    None
                ),
                (
                    "0000:80:03.4",
                    Some("pci0000:80"),
                    HardwareRole::RootPort,
                    None
                ),
                (
                    "0000:01:00.0",
                    Some("0000:00:01.1"),
                    HardwareRole::Endpoint,
                    Some(HardwareKind::Gpu)
                ),
                (
                    "0000:85:00.0",
                    Some("0000:80:03.4"),
                    HardwareRole::Endpoint,
                    Some(HardwareKind::Rdma)
                ),
            ]
        );
        let gpu = &nodes[4];
        assert_eq!(
            gpu.link.unwrap().current,
            Some(PcieLink {
                speed_gts: 8.0,
                width: 16
            })
        );
        assert_eq!(
            gpu.link.unwrap().max,
            Some(PcieLink {
                speed_gts: 32.0,
                width: 16
            })
        );
        assert_eq!(gpu.numa_node, None);
        assert_eq!(
            gpu.bars,
            [
                BarInfo {
                    index: 0,
                    bytes: 64 << 20,
                    prefetchable: false
                },
                BarInfo {
                    index: 1,
                    bytes: 256 << 20,
                    prefetchable: true
                },
            ]
        );
        assert_eq!(online_numa_nodes(&f.sys()), 1);
        assert!(
            (PcieLink {
                speed_gts: 5.0,
                width: 8
            }
            .bandwidth()
                - 4.0)
                .abs()
                < 1e-9
        );
        let switched = fixture::switched("pci-switched");
        let nodes = read_topology(&switched.sys());
        assert_eq!(nodes.iter().filter(|n| n.kind.is_some()).count(), 6);
        assert_eq!(
            nodes
                .iter()
                .filter(|n| n.role == HardwareRole::SwitchPort)
                .count(),
            8
        );
        assert_eq!(online_numa_nodes(&switched.sys()), 2);
    }
}
