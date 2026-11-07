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
//! | [`HardwareObservation`] | `pub` | `struct HardwareObservation` |
//! | [`HardwareSources`] | `pub` | `struct HardwareSources` |

mod devices;
mod handles;
pub(crate) mod observation;
mod paths;
mod pci;
pub use observation::{HardwareObservation, HardwareSources};

#[cfg(test)]
pub(crate) use observation::acquisition;
