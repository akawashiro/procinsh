//! Process history points interpreted by main.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`HistoryPoint`] | `pub(crate)` | `struct HistoryPoint` |

#[derive(Clone, Debug)]
pub(crate) struct HistoryPoint {
    pub(crate) timestamp: u64,
    pub(crate) cpu_percent: Option<f64>,
    pub(crate) rss_bytes: u64,
    pub(crate) vms_bytes: u64,
}

crate::ipc::codec::wire_struct!(HistoryPoint {
    timestamp,
    cpu_percent,
    rss_bytes,
    vms_bytes
});
