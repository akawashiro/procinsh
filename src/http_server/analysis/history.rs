//! Retain bounded process histories in main.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`append_history_observation`] | `pub` | `fn append_history_observation(history: &mut VecDeque<HistoryPoint>, observation: &ProcessObservation)` |
//!
//! Shared signature types: [`HistoryPoint`].

//! History observations use [`ProcessObservation`].

use crate::ipc::history::HistoryPoint;
use crate::ipc::model::process::ProcessObservation;
use std::collections::VecDeque;

#[derive(serde::Serialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::history::HistoryPoint")]
struct HistoryPointJson {
    pub timestamp: u64,
    pub cpu_percent: Option<f64>,
    pub rss_bytes: u64,
    pub vms_bytes: u64,
}
impl serde::Serialize for crate::ipc::history::HistoryPoint {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        HistoryPointJson::serialize(self, s)
    }
}

pub fn append_history_observation(
    history: &mut VecDeque<HistoryPoint>,
    observation: &ProcessObservation,
) {
    let cutoff = observation.timestamp.saturating_sub(60_000);
    history.push_back(HistoryPoint {
        timestamp: observation.timestamp,
        cpu_percent: observation.cpu_percent,
        rss_bytes: observation.rss_bytes,
        vms_bytes: observation.vms_bytes,
    });
    while history.front().is_some_and(|o| o.timestamp < cutoff) || history.len() > 601 {
        history.pop_front();
    }
}
