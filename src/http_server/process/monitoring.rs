//! Process observation and collector lifecycle.
mod history;
mod service;
pub use service::Monitoring;
use service::ProcessObservation;
pub use service::observation;
