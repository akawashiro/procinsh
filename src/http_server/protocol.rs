//! Convert native replies into main-owned values for HTTP analysis and output.
//!
//! # Interface
//! - [`MonitorEvent`] (`pub(super)`): `enum MonitorEvent`.
//! - [`MonitorResponse`] (`pub(super)`): `enum MonitorResponse`.
//! - [`convert_monitor_response`] (`pub(super)`): `fn convert_monitor_response(raw: crate::ipc::schema::MonitorResponse) -> anyhow::Result<MonitorResponse>`.
//!
use anyhow::Result;
use serde_json::Value;
#[derive(serde::Serialize, serde::Deserialize)]
pub(super) enum MonitorEvent {
    Observation(Value),
    Snapshot(Value),
    Activity(Value),
    Gap {
        dropped: u64,
        snapshot: Value,
    },
    #[serde(skip)]
    AcquiredSnapshot(std::sync::Arc<crate::ipc::model::system::SystemSnapshot>),
    #[serde(skip)]
    AcquisitionGap {
        dropped: u64,
        snapshot: std::sync::Arc<crate::ipc::model::system::SystemSnapshot>,
    },
}
#[derive(serde::Serialize, serde::Deserialize)]
pub(super) enum MonitorResponse {
    Data(Value),
    Started {
        session: u64,
        initial: MonitorEvent,
    },
    Event {
        event: Option<MonitorEvent>,
        closed: bool,
    },
    Closed,
}

fn convert_monitor_event(raw: crate::ipc::schema::MonitorEvent) -> Result<MonitorEvent> {
    use crate::ipc::schema;
    Ok(match raw {
        schema::MonitorEvent::Observation(value) => {
            MonitorEvent::Observation(serde_json::to_value(&*value)?)
        }
        schema::MonitorEvent::Activity(value) => {
            MonitorEvent::Activity(serde_json::to_value(&*value)?)
        }
        schema::MonitorEvent::AcquiredSnapshot(value) => MonitorEvent::AcquiredSnapshot(value),
        schema::MonitorEvent::AcquisitionGap { dropped, snapshot } => {
            MonitorEvent::AcquisitionGap { dropped, snapshot }
        }
    })
}

pub(super) fn convert_monitor_response(
    raw: crate::ipc::schema::MonitorResponse,
) -> Result<MonitorResponse> {
    use crate::ipc::schema::{self, AcquiredData};
    Ok(match raw {
        schema::MonitorResponse::Data(value) => MonitorResponse::Data(match value {
            AcquiredData::Processes(v) => serde_json::to_value(v)?,
            AcquiredData::Observation(v) => serde_json::to_value(v)?,
            AcquiredData::Maps(v) => serde_json::to_value(v)?,
            AcquiredData::Environment(v) => serde_json::to_value(v)?,
            AcquiredData::Auxv(v) => serde_json::to_value(v)?,
            AcquiredData::Fds(v) => serde_json::to_value(v)?,
        }),
        schema::MonitorResponse::Elf(_) => anyhow::bail!("unexpected mapped file reply"),
        schema::MonitorResponse::Started { session, initial } => MonitorResponse::Started {
            session,
            initial: convert_monitor_event(initial)?,
        },
        schema::MonitorResponse::Event { event: raw, closed } => MonitorResponse::Event {
            event: raw.map(convert_monitor_event).transpose()?,
            closed,
        },
        schema::MonitorResponse::Closed => MonitorResponse::Closed,
    })
}
