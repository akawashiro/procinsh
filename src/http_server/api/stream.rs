//! Encode observation events and build server-sent-event responses.
//!
//! # Interface
//! - [`encode_sse_events`] (`pub(super)`): `fn encode_sse_events( event: MonitorEvent, delivery: &mut SnapshotEncoder, ) -> (Vec<(&'static str, String)>, Option<Arc<Value>>)`.
//! - [`build_sse_response`] (`pub(super)`): `fn build_sse_response( state: Arc<AppState>, mut session: ObservationSession, ) -> Response`.
//!
//! Supporting types: [`AppState`], [`Arc`], [`MonitorEvent`], [`ObservationSession`], [`Response`], [`SnapshotEncoder`], [`Value`].

use super::super::{AppState, client::ObservationSession, delivery::SnapshotEncoder};
use crate::http_server::protocol::MonitorEvent;
use axum::response::{
    IntoResponse, Response, Sse,
    sse::{Event, KeepAlive},
};
use serde_json::Value;
use std::{
    convert::Infallible,
    sync::Arc,
    time::{Duration, Instant},
};

// HTTP formatting, JSON output, delta selection and SSE recovery all happen
// in the HTTP worker. IPC carries complete structured observations.
/// SSE events for `event`, and the complete snapshot it carries, if any.
pub(super) fn encode_sse_events(
    event: MonitorEvent,
    delivery: &mut SnapshotEncoder,
) -> (Vec<(&'static str, String)>, Option<Arc<Value>>) {
    match event {
        MonitorEvent::AcquiredSnapshot(_) | MonitorEvent::AcquisitionGap { .. } => {
            unreachable!("acquisition snapshots must be processed before HTTP output")
        }
        MonitorEvent::Observation(data) => (vec![("observation", data.to_string())], None),
        MonitorEvent::Activity(data) => (vec![("activity", data.to_string())], None),
        MonitorEvent::Snapshot(data) => {
            let data = Arc::new(data);
            let encoded = delivery
                .encode(data.clone(), false, Instant::now())
                .unwrap();
            (vec![("snapshot", encoded)], Some(data))
        }
        MonitorEvent::Gap { dropped, snapshot } => {
            let snapshot = Arc::new(snapshot);
            let encoded = delivery
                .encode(snapshot.clone(), true, Instant::now())
                .unwrap();
            (
                vec![
                    (
                        "gap",
                        serde_json::json!({"dropped_frames":dropped}).to_string(),
                    ),
                    ("snapshot", encoded),
                ],
                Some(snapshot),
            )
        }
    }
}

pub(super) fn build_sse_response(
    state: Arc<AppState>,
    mut session: ObservationSession,
) -> Response {
    let stream = async_stream::stream! {
        let mut delivery = SnapshotEncoder::default();
        let mut event = session.initial.take();
        let mut closed = false;
        loop {
            let idle = event.is_none();
            if let Some(event) = event.take() {
                for (name, data) in encode_sse_events(event, &mut delivery).0 {
                    let path = if name == "observation" { "/api/processes/events" } else { "/api/system/events" };
                    log::debug!("SSE {path} event={name} payload_bytes={}", data.len());
                    yield Ok::<_, Infallible>(Event::default().event(name).data(data));
                }
            }
            if closed || state.stopped() { break; }
            if idle { tokio::time::sleep(Duration::from_millis(100)).await; }
            match session.poll().await {
                Ok(next) => { (event, closed) = next; }
                Err(error) => { log::debug!("observation IPC ended: {error:#}"); break; }
            }
        }
        drop(session);
    };
    Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
        .into_response()
}
