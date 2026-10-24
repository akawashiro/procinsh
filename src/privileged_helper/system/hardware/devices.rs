//! Serialized hardware devices, enriched with GPU and RDMA identity.

use super::pci::{BarInfo, HardwareKind, HardwareRole, PciNode, PcieLinkInfo, names, text};
use serde::Serialize;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(in crate::privileged_helper::system) struct GpuInfo {
    /// `nvidia`, `amd`, `intel` or the PCI vendor ID.
    pub(in crate::privileged_helper::system) vendor: String,
    pub(in crate::privileged_helper::system) model: Option<String>,
    pub(in crate::privileged_helper::system) uuid: Option<String>,
    /// The NVIDIA device minor of `/dev/nvidiaN`.
    pub(in crate::privileged_helper::system) minor: Option<u32>,
    /// DRM nodes such as `card1` and `renderD128`.
    pub(in crate::privileged_helper::system) drm: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(in crate::privileged_helper::system) struct RdmaDeviceInfo {
    pub(in crate::privileged_helper::system) name: String,
    pub(in crate::privileged_helper::system) hca_type: Option<String>,
    pub(in crate::privileged_helper::system) fw_ver: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub(in crate::privileged_helper::system) struct HardwareDevice {
    /// A PCI function address such as `0000:85:00.0`, or a host bridge such as `pci0000:80`.
    pub(in crate::privileged_helper::system) address: String,
    pub(in crate::privileged_helper::system) parent: Option<String>,
    pub(in crate::privileged_helper::system) role: HardwareRole,
    pub(in crate::privileged_helper::system) kind: Option<HardwareKind>,
    pub(in crate::privileged_helper::system) label: String,
    /// The 24-bit class code as `0x030000`.
    pub(in crate::privileged_helper::system) class: Option<String>,
    pub(in crate::privileged_helper::system) vendor_id: Option<String>,
    pub(in crate::privileged_helper::system) device_id: Option<String>,
    pub(in crate::privileged_helper::system) driver: Option<String>,
    pub(in crate::privileged_helper::system) numa_node: Option<u32>,
    /// Null for host bridges and runtime-suspended devices.
    pub(in crate::privileged_helper::system) link: Option<PcieLinkInfo>,
    pub(in crate::privileged_helper::system) power_state: Option<String>,
    pub(in crate::privileged_helper::system) runtime_status: Option<String>,
    pub(in crate::privileged_helper::system) iommu_group: Option<u32>,
    pub(in crate::privileged_helper::system) bars: Vec<BarInfo>,
    pub(in crate::privileged_helper::system) gpu: Option<GpuInfo>,
    pub(in crate::privileged_helper::system) rdma: Vec<RdmaDeviceInfo>,
    /// Network interfaces of the device.
    pub(in crate::privileged_helper::system) netdevs: Vec<String>,
}

/// `Key: value` lines of `/proc/driver/nvidia/gpus/<address>/information`.
fn nvidia_information(proc: &Path, address: &str) -> Vec<(String, String)> {
    text(
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

fn short(address: &str) -> &str {
    address.strip_prefix("0000:").unwrap_or(address)
}

pub(super) fn device(node: &PciNode, proc: &Path) -> HardwareDevice {
    let hex = |value: Option<u32>, width: usize| value.map(|v| format!("0x{v:0width$x}"));
    let gpu = (node.kind == Some(HardwareKind::Gpu)).then(|| {
        let info = nvidia_information(proc, &node.address);
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
            drm: names(&node.path.join("drm"))
                .into_iter()
                .filter(|n| n.starts_with("card") || n.starts_with("renderD"))
                .filter(|n| !n.contains('-'))
                .collect(),
        }
    });
    let rdma: Vec<_> = names(&node.path.join("infiniband"))
        .into_iter()
        .map(|name| {
            let dir = node.path.join("infiniband").join(&name);
            RdmaDeviceInfo {
                hca_type: text(&dir.join("hca_type")),
                fw_ver: text(&dir.join("fw_ver")),
                name,
            }
        })
        .collect();
    let label = match (node.role, node.kind) {
        (HardwareRole::HostBridge, _) => {
            format!("Host bridge {}", node.address.trim_start_matches("pci"))
        }
        (HardwareRole::RootPort, _) => format!("Root port {}", short(&node.address)),
        (HardwareRole::SwitchPort, _) => format!("Switch port {}", short(&node.address)),
        (_, Some(HardwareKind::Gpu)) => gpu
            .as_ref()
            .and_then(|g| g.model.clone())
            .unwrap_or_else(|| format!("GPU {}", short(&node.address))),
        (_, Some(HardwareKind::Rdma)) if !rdma.is_empty() => rdma
            .iter()
            .map(|d| d.name.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        (_, Some(HardwareKind::Rdma)) => format!("RDMA NIC {}", short(&node.address)),
        (_, Some(HardwareKind::Accelerator)) => format!("Accelerator {}", short(&node.address)),
        (_, Some(HardwareKind::NvSwitch)) => format!("NVSwitch {}", short(&node.address)),
        (_, None) => short(&node.address).to_owned(),
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
            names(&node.path.join("net"))
        } else {
            vec![]
        },
    }
}
