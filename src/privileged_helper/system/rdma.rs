//! RDMA queue pairs, connection manager IDs and port addresses, observed read-only
//! through RDMA netlink, rtnetlink IPoIB neighbors and sysfs.
//!
//! RDMA netlink is queried only when `class/infiniband` under the sysfs root lists a
//! device, so a host without RDMA never autoloads `ib_core`. Verbs data transfers
//! bypass the kernel; queue pairs carry no per-QP traffic observation.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`RdmaRelation`] | `pub(super)` | `struct RdmaRelation` |
//! | [`RdmaPortInfo`] | `pub(super)` | `struct RdmaPortInfo` |
//! | [`observe`] | `pub(super)` | `fn observe(sys: &`[`Path`](std::path::Path)`, processes: &`[`HashMap`](std::collections::HashMap)`<`[`i32`]`, `[`ProcessId`](crate::privileged_helper::process::ProcessId)`>, deadline: `[`Instant`](std::time::Instant)`) -> `[`observation::RdmaObservation`] |
//!
//! [`RdmaRelation`] refers to [`relations::RdmaEndpoint`], [`relations::RdmaCmInfo`],
//! [`RdmaAddress`](crate::ipc::rdma::RdmaAddress) and [`IbGid`](crate::ipc::rdma::IbGid).
//! Remote GIDs of IP destinations come from [`super::ipoib::ipoib_gids`].

mod nldev;
mod observation;
mod ports;
mod relations;
pub(super) use observation::observe;
pub(super) use ports::RdmaPortInfo;
pub(super) use relations::RdmaRelation;
#[cfg(test)]
pub(super) use relations::test_relation;
