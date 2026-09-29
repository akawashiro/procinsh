//! Process observation and collector lifecycle.
mod history;
mod service;
pub(in crate::http_server) use service::Monitoring;
use service::ProcessObservation;
pub(super) use service::observation;
