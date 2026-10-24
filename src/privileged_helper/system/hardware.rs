//! GPUs, RDMA NICs, accelerators and NVSwitches with the PCIe bridges between them,
//! the paths between devices, and the processes holding their character devices.
//!
//! Everything comes from sysfs and procfs without privileges; `config` space and
//! vendor libraries are not used, so no device is woken or modified.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind |
//! | --- | --- | --- |
//! | [`HardwareObservation`] | `pub(super)` | `struct HardwareObservation` |
//! | [`HardwareSources`] | `pub(super)` | `struct HardwareSources` |
//! | [`HardwareDevice`] | `pub(super)` | `struct HardwareDevice` |
//! | [`HardwarePath`] | `pub(super)` | `struct HardwarePath` |
//! | [`DeviceHandle`] | `pub(super)` | `struct DeviceHandle` |
//!
//! [`HardwareObservation::handles`] returns a [`handles::DeviceHandleCollector`].

mod devices;
mod handles;
mod observation;
mod paths;
mod pci;
pub(super) use devices::HardwareDevice;
pub(super) use handles::DeviceHandle;
pub(super) use observation::{HardwareObservation, HardwareSources};
pub(super) use paths::HardwarePath;
