//! GPUs, RDMA NICs, accelerators and NVSwitches with the PCIe bridges between them,
//! the paths between devices, and the processes holding their character devices.
//!
//! Interprets helper-acquired sysfs/procfs text and links entirely in memory.
//! This module opens no host files; PCI config space and vendor libraries are unused.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`HardwareDevice`] | `pub` | `struct HardwareDevice` |
//! | [`DeviceHandle`] | `pub` | `struct DeviceHandle` |
//! | [`HardwareObservation`] | `pub` | `struct HardwareObservation` |
//! | [`HardwareSources`] | `pub` | `struct HardwareSources` |
//! | [`HardwarePath`] | `pub` | `struct HardwarePath` |

mod devices;
mod handles;
mod observation;
mod paths;
mod pci;
pub use devices::HardwareDevice;
pub use handles::DeviceHandle;
pub use observation::{HardwareObservation, HardwareSources};
pub use paths::HardwarePath;

#[cfg(test)]
pub(crate) use observation::acquisition;
