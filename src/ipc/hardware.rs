//! Raw hardware attributes acquired by helper and interpreted by the HTTP server.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`HardwareDevice`] | `pub(crate)` | `struct HardwareDevice` |
//! | [`RdmaDeviceInfo`] | `pub(crate)` | `struct RdmaDeviceInfo` |
//! | [`GpuInfo`] | `pub(crate)` | `struct GpuInfo` |
//! | [`DeviceHandle`] | `pub(crate)` | `struct DeviceHandle` |
//! | [`DeviceHandleKind`] | `pub(crate)` | `enum DeviceHandleKind` |
//! | [`HardwarePath`] | `pub(crate)` | `struct HardwarePath` |
//! | [`PathClass`] | `pub(crate)` | `enum PathClass` |
//! | [`BarInfo`] | `pub(crate)` | `struct BarInfo` |
//! | [`PcieLinkInfo`] | `pub(crate)` | `struct PcieLinkInfo` |
//! | [`PcieLink`] | `pub(crate)` | `struct PcieLink` |
//! | [`HardwareKind`] | `pub(crate)` | `enum HardwareKind` |
//! | [`HardwareRole`] | `pub(crate)` | `enum HardwareRole` |
//! | [`HardwareFiles`] | `pub(crate)` | `struct HardwareFiles` |

use crate::ipc::schema::ProcessId;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HardwareDevice {
    /// A PCI function address such as `0000:85:00.0`, or a host bridge such as `pci0000:80`.
    pub(crate) address: String,
    pub(crate) parent: Option<String>,
    pub(crate) role: HardwareRole,
    pub(crate) kind: Option<HardwareKind>,
    pub(crate) label: String,
    /// The 24-bit class code as `0x030000`.
    pub(crate) class: Option<String>,
    pub(crate) vendor_id: Option<String>,
    pub(crate) device_id: Option<String>,
    pub(crate) driver: Option<String>,
    pub(crate) numa_node: Option<u32>,
    /// Null for host bridges and runtime-suspended devices.
    pub(crate) link: Option<PcieLinkInfo>,
    pub(crate) power_state: Option<String>,
    pub(crate) runtime_status: Option<String>,
    pub(crate) iommu_group: Option<u32>,
    pub(crate) bars: Vec<BarInfo>,
    pub(crate) gpu: Option<GpuInfo>,
    pub(crate) rdma: Vec<RdmaDeviceInfo>,
    /// Network interfaces of the device.
    pub(crate) netdevs: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RdmaDeviceInfo {
    pub(crate) name: String,
    pub(crate) hca_type: Option<String>,
    pub(crate) fw_ver: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct GpuInfo {
    /// `nvidia`, `amd`, `intel` or the PCI vendor ID.
    pub(crate) vendor: String,
    pub(crate) model: Option<String>,
    pub(crate) uuid: Option<String>,
    /// The NVIDIA device minor of `/dev/nvidiaN`.
    pub(crate) minor: Option<u32>,
    /// DRM nodes such as `card1` and `renderD128`.
    pub(crate) drm: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DeviceHandle {
    pub(crate) id: String,
    pub(crate) process_id: ProcessId,
    pub(crate) device: String,
    pub(crate) kind: DeviceHandleKind,
    /// The lowest descriptor number.
    pub(crate) fd: u32,
    pub(crate) fd_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum DeviceHandleKind {
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

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HardwarePath {
    pub(crate) id: String,
    pub(crate) a: String,
    pub(crate) b: String,
    pub(crate) class: PathClass,
    /// Nodes between the ends, in order from `a` to `b`.
    pub(crate) via: Vec<String>,
    /// The slowest negotiated link on the path; null when no link speed is known.
    pub(crate) bottleneck: Option<PcieLink>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PathClass {
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

crate::ipc::codec::wire_struct!(HardwareDevice {
    address,
    parent,
    role,
    kind,
    label,
    class,
    vendor_id,
    device_id,
    driver,
    numa_node,
    link,
    power_state,
    runtime_status,
    iommu_group,
    bars,
    gpu,
    rdma,
    netdevs
});

crate::ipc::codec::wire_struct!(RdmaDeviceInfo {
    name,
    hca_type,
    fw_ver
});

crate::ipc::codec::wire_struct!(GpuInfo {
    vendor,
    model,
    uuid,
    minor,
    drm
});

crate::ipc::codec::wire_struct!(DeviceHandle {
    id,
    process_id,
    device,
    kind,
    fd,
    fd_count
});

crate::ipc::codec::wire_enum!(DeviceHandleKind{0=>Nvidia,1=>DrmRender,2=>DrmCard,3=>Uverbs,4=>Umad,5=>Accel});

crate::ipc::codec::wire_struct!(HardwarePath {
    id,
    a,
    b,
    class,
    via,
    bottleneck
});

crate::ipc::codec::wire_enum!(PathClass{0=>Pix,1=>Pxb,2=>Phb,3=>Node,4=>Sys});

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BarInfo {
    pub(crate) index: u32,
    pub(crate) bytes: u64,
    pub(crate) prefetchable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PcieLinkInfo {
    pub(crate) current: Option<PcieLink>,
    pub(crate) max: Option<PcieLink>,
}

/// The negotiated or maximum link of a device to its upstream port.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PcieLink {
    pub(crate) speed_gts: f64,
    pub(crate) width: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum HardwareKind {
    Gpu,
    /// An RDMA-capable NIC (InfiniBand or RoCE).
    Rdma,
    /// A processing accelerator (PCI class 0x12).
    Accelerator,
    NvSwitch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HardwareRole {
    HostBridge,
    RootPort,
    SwitchPort,
    Endpoint,
}

crate::ipc::codec::wire_struct!(PcieLink { speed_gts, width });
crate::ipc::codec::wire_struct!(PcieLinkInfo { current, max });
crate::ipc::codec::wire_struct!(BarInfo {
    index,
    bytes,
    prefetchable
});
crate::ipc::codec::wire_enum!(HardwareRole { 0 => HostBridge, 1 => RootPort, 2 => SwitchPort, 3 => Endpoint });
crate::ipc::codec::wire_enum!(HardwareKind { 0 => Gpu, 1 => Rdma, 2 => Accelerator, 3 => NvSwitch });

/// Bounded text attributes, directory names and resolved links from fixed sysfs/procfs roots.
#[derive(Clone, Debug, Default)]
pub(crate) struct HardwareFiles {
    pub(crate) truncated: bool,
    pub(crate) texts: Vec<(String, String)>,
    pub(crate) directories: Vec<(String, Vec<String>)>,
    pub(crate) links: Vec<(String, String)>,
}
crate::ipc::codec::wire_struct!(HardwareFiles {
    truncated,
    texts,
    directories,
    links
});
