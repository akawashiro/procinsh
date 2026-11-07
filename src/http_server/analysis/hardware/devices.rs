//! Serialized hardware devices, enriched with GPU and RDMA identity.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`build_hardware_device`] | `pub` | `fn build_hardware_device(sources: &`[`HardwareSources`]`, node: &`[`PciNode`]`, proc: &`[`Path`]`) -> `[`HardwareDevice`] |

use super::observation::HardwareSources;
use crate::ipc::hardware::{GpuInfo, HardwareDevice, RdmaDeviceInfo};
use std::path::Path;
use {
    super::pci::PciNode,
    crate::ipc::hardware::{BarInfo, HardwareKind, HardwareRole, PcieLinkInfo},
};

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::hardware::GpuInfo")]
struct GpuInfoJson {
    /// `nvidia`, `amd`, `intel` or the PCI vendor ID.
    pub vendor: String,
    pub model: Option<String>,
    pub uuid: Option<String>,
    /// The NVIDIA device minor of `/dev/nvidiaN`.
    pub minor: Option<u32>,
    /// DRM nodes such as `card1` and `renderD128`.
    pub drm: Vec<String>,
}
impl serde::Serialize for crate::ipc::hardware::GpuInfo {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        GpuInfoJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::hardware::GpuInfo {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        GpuInfoJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::hardware::RdmaDeviceInfo")]
struct RdmaDeviceInfoJson {
    pub name: String,
    pub hca_type: Option<String>,
    pub fw_ver: Option<String>,
}
impl serde::Serialize for crate::ipc::hardware::RdmaDeviceInfo {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        RdmaDeviceInfoJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::hardware::RdmaDeviceInfo {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        RdmaDeviceInfoJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::hardware::HardwareDevice")]
struct HardwareDeviceJson {
    /// A PCI function address such as `0000:85:00.0`, or a host bridge such as `pci0000:80`.
    pub address: String,
    pub parent: Option<String>,
    pub role: HardwareRole,
    pub kind: Option<HardwareKind>,
    pub label: String,
    /// The 24-bit class code as `0x030000`.
    pub class: Option<String>,
    pub vendor_id: Option<String>,
    pub device_id: Option<String>,
    pub driver: Option<String>,
    pub numa_node: Option<u32>,
    /// Null for host bridges and runtime-suspended devices.
    pub link: Option<PcieLinkInfo>,
    pub power_state: Option<String>,
    pub runtime_status: Option<String>,
    pub iommu_group: Option<u32>,
    pub bars: Vec<BarInfo>,
    pub gpu: Option<GpuInfo>,
    pub rdma: Vec<RdmaDeviceInfo>,
    /// Network interfaces of the device.
    pub netdevs: Vec<String>,
}
impl serde::Serialize for crate::ipc::hardware::HardwareDevice {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        HardwareDeviceJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::hardware::HardwareDevice {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        HardwareDeviceJson::deserialize(d)
    }
}

/// `Key: value` lines of `/proc/driver/nvidia/gpus/<address>/information`.
fn nvidia_information(
    sources: &HardwareSources,
    proc: &Path,
    address: &str,
) -> Vec<(String, String)> {
    sources
        .text(
            &proc
                .join("driver/nvidia/gpus")
                .join(address)
                .join("information"),
        )
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            Some((key.trim().to_owned(), value.trim().to_owned()))
        })
        .collect()
}

fn shorten_pci_address(address: &str) -> &str {
    address.strip_prefix("0000:").unwrap_or(address)
}

pub fn build_hardware_device(
    sources: &HardwareSources,
    node: &PciNode,
    proc: &Path,
) -> HardwareDevice {
    let hex = |value: Option<u32>, width: usize| value.map(|v| format!("0x{v:0width$x}"));
    let gpu = (node.kind == Some(HardwareKind::Gpu)).then(|| {
        let info = nvidia_information(sources, proc, &node.address);
        let field = |key: &str| info.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
        GpuInfo {
            vendor: match node.vendor_id {
                Some(0x10de) => "nvidia".into(),
                Some(0x1002) => "amd".into(),
                Some(0x8086) => "intel".into(),
                other => hex(other, 4).unwrap_or_default(),
            },
            model: field("Model"),
            uuid: field("GPU UUID"),
            minor: field("Device Minor").and_then(|m| m.parse().ok()),
            drm: sources
                .names(&node.path.join("drm"))
                .into_iter()
                .filter(|n| n.starts_with("card") || n.starts_with("renderD"))
                .filter(|n| !n.contains('-'))
                .collect(),
        }
    });
    let rdma: Vec<_> = sources
        .names(&node.path.join("infiniband"))
        .into_iter()
        .map(|name| {
            let dir = node.path.join("infiniband").join(&name);
            RdmaDeviceInfo {
                hca_type: sources.text(&dir.join("hca_type")),
                fw_ver: sources.text(&dir.join("fw_ver")),
                name,
            }
        })
        .collect();
    let label = match (node.role, node.kind) {
        (HardwareRole::HostBridge, _) => {
            format!("Host bridge {}", node.address.trim_start_matches("pci"))
        }
        (HardwareRole::RootPort, _) => format!("Root port {}", shorten_pci_address(&node.address)),
        (HardwareRole::SwitchPort, _) => {
            format!("Switch port {}", shorten_pci_address(&node.address))
        }
        (_, Some(HardwareKind::Gpu)) => gpu
            .as_ref()
            .and_then(|g| g.model.clone())
            .unwrap_or_else(|| format!("GPU {}", shorten_pci_address(&node.address))),
        (_, Some(HardwareKind::Rdma)) if !rdma.is_empty() => rdma
            .iter()
            .map(|d| d.name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        (_, Some(HardwareKind::Rdma)) => format!("RDMA NIC {}", shorten_pci_address(&node.address)),
        (_, Some(HardwareKind::Accelerator)) => {
            format!("Accelerator {}", shorten_pci_address(&node.address))
        }
        (_, Some(HardwareKind::NvSwitch)) => {
            format!("NVSwitch {}", shorten_pci_address(&node.address))
        }
        (_, None) => shorten_pci_address(&node.address).to_owned(),
    };
    HardwareDevice {
        address: node.address.clone(),
        parent: node.parent.clone(),
        role: node.role,
        kind: node.kind,
        label,
        class: hex(node.class, 6),
        vendor_id: hex(node.vendor_id, 4),
        device_id: hex(node.device_id, 4),
        driver: node.driver.clone(),
        numa_node: node.numa_node,
        link: node.link,
        power_state: node.power_state.clone(),
        runtime_status: node.runtime_status.clone(),
        iommu_group: node.iommu_group,
        bars: node.bars.clone(),
        gpu,
        rdma,
        netdevs: if node.role == HardwareRole::Endpoint {
            sources.names(&node.path.join("net"))
        } else {
            vec![]
        },
    }
}
