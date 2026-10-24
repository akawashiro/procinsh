use super::super::{AppState, client::ObservationSession, delivery::SnapshotEncoder};
use axum::response::{
    IntoResponse, Response, Sse,
    sse::{Event, KeepAlive},
};
use procinsh::protocol::MonitorEvent;
use std::{
    convert::Infallible,
    sync::Arc,
    time::{Duration, Instant},
};

// HTTP formatting, JSON output, delta selection and SSE recovery all happen
// in the unprivileged worker. IPC carries complete structured observations.
fn encode(event: MonitorEvent, delivery: &mut SnapshotEncoder) -> Vec<(&'static str, String)> {
    match event {
        MonitorEvent::Observation(data) => vec![("observation", data.to_string())],
        MonitorEvent::Activity(data) => vec![("activity", data.to_string())],
        MonitorEvent::Snapshot(data) => vec![(
            "snapshot",
            delivery
                .encode(Arc::new(data), false, Instant::now())
                .unwrap(),
        )],
        MonitorEvent::Gap { dropped, snapshot } => vec![
            (
                "gap",
                serde_json::json!({"dropped_frames":dropped}).to_string(),
            ),
            (
                "snapshot",
                delivery
                    .encode(Arc::new(snapshot), true, Instant::now())
                    .unwrap(),
            ),
        ],
    }
}

pub(super) fn response(state: Arc<AppState>, mut session: ObservationSession) -> Response {
    let stream = async_stream::stream! {
        let mut delivery = SnapshotEncoder::default();
        let mut event = session.initial.take();
        let mut closed = false;
        loop {
            let idle = event.is_none();
            if let Some(event) = event.take() {
                for (name, data) in encode(event, &mut delivery) {
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
