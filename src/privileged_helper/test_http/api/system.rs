use super::super::{
    AppState,
    system::{SnapshotEncoder, SubscribeError, SystemMonitorEvent},
};
use axum::{
    extract::State,
    http::StatusCode,
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
};
use serde_json::json;
use std::{
    convert::Infallible,
    sync::Arc,
    time::{Duration, Instant},
};

pub(super) async fn stream_system_events(
    State(s): State<Arc<AppState>>,
) -> Result<Response, StatusCode> {
    use crate::privileged_helper::system::{
        encode_snapshot_encoder, snapshot_system_monitor, stopped_system_monitor,
        subscribe_system_monitor,
    };

    let mut subscription =
        subscribe_system_monitor(&s.system_monitor).map_err(|error| match error {
            SubscribeError::Stopped => StatusCode::SERVICE_UNAVAILABLE,
            SubscribeError::TooManySubscribers => StatusCode::TOO_MANY_REQUESTS,
        })?;
    let stream = async_stream::stream! {
        let mut delivery = SnapshotEncoder::default();
        let initial = encode_snapshot_encoder(&mut delivery, subscription.initial.clone(), true, Instant::now()).unwrap();
        log::debug!("SSE /api/system/events event=snapshot payload_bytes={}", initial.len());
        yield Ok::<_,Infallible>(Event::default().event("snapshot").data(initial));
        loop{if stopped_system_monitor(&s.system_monitor){break;}
            match tokio::time::timeout(Duration::from_secs(1),subscription.receiver.recv()).await {
                Ok(Ok(message)) => {
                    let (event, data) = match message {
                        SystemMonitorEvent::Snapshot(data) => ("snapshot", encode_snapshot_encoder(&mut delivery, data, false, Instant::now()).unwrap()),
                        SystemMonitorEvent::Activity(data) => ("activity", serde_json::to_string(&*data).unwrap()),
                    };
                    log::debug!("SSE /api/system/events event={event} payload_bytes={}", data.len());
                    yield Ok(Event::default().event(event).data(data));
                },
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(n))) => {
                    let data = json!({"dropped_frames":n}).to_string();
                    log::debug!("SSE /api/system/events event=gap payload_bytes={} dropped_frames={n}", data.len());
                    yield Ok(Event::default().event("gap").data(data));
                    let snapshot = encode_snapshot_encoder(&mut delivery, snapshot_system_monitor(&s.system_monitor), true, Instant::now()).unwrap();
                    log::debug!("SSE /api/system/events event=snapshot payload_bytes={}", snapshot.len());
                    yield Ok(Event::default().event("snapshot").data(snapshot));
                },
                Ok(Err(_))=>break,Err(_)=>{}
            }
        }
    };
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
        .into_response())
}
