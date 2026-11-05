//! Raw hardware attributes acquired by helper and interpreted by the HTTP server.

//!
//! # Interface
//! - [`HardwareFiles`] (`pub`): `struct HardwareFiles`.
//! - [`HardwareDevice`] (`pub`): `struct HardwareDevice`.
//! - [`RdmaDeviceInfo`] (`pub`): `struct RdmaDeviceInfo`.
//! - [`GpuInfo`] (`pub`): `struct GpuInfo`.
//! - [`DeviceHandle`] (`pub`): `struct DeviceHandle`.
//! - [`DeviceHandleKind`] (`pub`): `enum DeviceHandleKind`.
//! - [`HardwarePath`] (`pub`): `struct HardwarePath`.
//! - [`PathClass`] (`pub`): `enum PathClass`.
//! - [`BarInfo`] (`pub`): `struct BarInfo`.
//! - [`PcieLinkInfo`] (`pub`): `struct PcieLinkInfo`.
//! - [`PcieLink`] (`pub`): `struct PcieLink`.
//! - [`HardwareKind`] (`pub`): `enum HardwareKind`.
//! - [`HardwareRole`] (`pub`): `enum HardwareRole`.
use crate::ipc::schema::ProcessId;

#[derive(Clone, Debug, PartialEq)]
pub struct HardwareDevice {
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RdmaDeviceInfo {
    pub name: String,
    pub hca_type: Option<String>,
    pub fw_ver: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GpuInfo {
    /// `nvidia`, `amd`, `intel` or the PCI vendor ID.
    pub vendor: String,
    pub model: Option<String>,
    pub uuid: Option<String>,
    /// The NVIDIA device minor of `/dev/nvidiaN`.
    pub minor: Option<u32>,
    /// DRM nodes such as `card1` and `renderD128`.
    pub drm: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceHandle {
    pub id: String,
    pub process_id: ProcessId,
    pub device: String,
    pub kind: DeviceHandleKind,
    /// The lowest descriptor number.
    pub fd: u32,
    pub fd_count: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DeviceHandleKind {
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
pub struct HardwarePath {
    pub id: String,
    pub a: String,
    pub b: String,
    pub class: PathClass,
    /// Nodes between the ends, in order from `a` to `b`.
    pub via: Vec<String>,
    /// The slowest negotiated link on the path; null when no link speed is known.
    pub bottleneck: Option<PcieLink>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathClass {
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
pub struct BarInfo {
    pub index: u32,
    pub bytes: u64,
    pub prefetchable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PcieLinkInfo {
    pub current: Option<PcieLink>,
    pub max: Option<PcieLink>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PcieLink {
    pub speed_gts: f64,
    pub width: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HardwareKind {
    Gpu,
    /// An RDMA-capable NIC (InfiniBand or RoCE).
    Rdma,
    /// A processing accelerator (PCI class 0x12).
    Accelerator,
    NvSwitch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HardwareRole {
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
pub struct HardwareFiles {
    pub truncated: bool,
    pub texts: Vec<(String, String)>,
    pub directories: Vec<(String, Vec<String>)>,
    pub links: Vec<(String, String)>,
}
crate::ipc::codec::wire_struct!(HardwareFiles {
    truncated,
    texts,
    directories,
    links
});
