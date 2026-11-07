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
//! | [`collect_rdma_observation`] | `pub(super)` | `fn collect_rdma_observation(sys: &`[`Path`](std::path::Path)`, processes: &`[`HashMap`](std::collections::HashMap)`<`[`i32`]`, `[`ProcessId`](crate::ipc::schema::ProcessId)`>, deadline: `[`Instant`](std::time::Instant)`) -> `[`observation::RdmaObservation`] |
//!
//! [`crate::ipc::rdma::RdmaRelation`] refers to [`crate::ipc::rdma::RdmaEndpoint`], [`crate::ipc::rdma::RdmaCmInfo`],
//! [`RdmaAddress`](crate::ipc::rdma::RdmaAddress) and [`IbGid`](crate::ipc::rdma::IbGid).
//! Remote GIDs of IP destinations come from [`super::ipoib::ipoib_gids`].

mod nldev;
mod observation;
mod ports;
mod relations;
pub(super) use observation::collect_rdma_observation;
#[cfg(test)]
pub(super) use relations::test_relation;
