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
//! | [`observation`] | `pub(crate)` | `mod observation` |
//! | [`finish_device_handle_collector`] | `pub(crate)` | `fn finish_device_handle_collector( state: `[`DeviceHandleCollector`](handles::DeviceHandleCollector)`, live: &`[`HashSet`](std::collections::HashSet)`<`[`ProcessId`](crate::ipc::schema::ProcessId)`>, ) -> `[`Vec`]`<`[`DeviceHandle`](crate::ipc::hardware::DeviceHandle)`>` |
//! | [`observe_device_handle_collector`] | `pub(crate)` | `fn observe_device_handle_collector( state: &mut `[`DeviceHandleCollector`](handles::DeviceHandleCollector)`, process: `[`ProcessId`](crate::ipc::schema::ProcessId)`, fd: u32, rdev: u64, )` |

mod devices;
mod handles;
pub(crate) mod observation;
mod paths;
mod pci;

#[cfg(test)]
pub(crate) use observation::acquisition;

pub(crate) use handles::{finish_device_handle_collector, observe_device_handle_collector};
