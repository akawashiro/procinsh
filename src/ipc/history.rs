//! Process history points interpreted by main.

//!
//! # Interface
//! - [`HistoryPoint`] (`pub`): `struct HistoryPoint`.
#[derive(Clone, Debug)]
pub struct HistoryPoint {
    pub timestamp: u64,
    pub cpu_percent: Option<f64>,
    pub rss_bytes: u64,
    pub vms_bytes: u64,
}

crate::ipc::codec::wire_struct!(HistoryPoint {
    timestamp,
    cpu_percent,
    rss_bytes,
    vms_bytes
});
