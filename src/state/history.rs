use super::ProcessObservation;
use serde::Serialize;
use std::collections::VecDeque;

#[derive(Clone, Debug, Serialize)]
pub struct HistoryPoint {
    pub timestamp: u64,
    pub cpu_percent: Option<f64>,
    pub rss_bytes: u64,
    pub vms_bytes: u64,
}

pub fn push(history: &mut VecDeque<HistoryPoint>, observation: &ProcessObservation) {
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
