use super::super::{
    AppState,
    system_monitoring::{SubscribeError, SystemEvent},
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
use std::{convert::Infallible, sync::Arc, time::Duration};
pub(super) async fn events(State(s): State<Arc<AppState>>) -> Result<Response, StatusCode> {
    let mut subscription = s.system.subscribe().map_err(|error| match error {
        SubscribeError::Stopped => StatusCode::SERVICE_UNAVAILABLE,
        SubscribeError::TooManySubscribers => StatusCode::TOO_MANY_REQUESTS,
    })?;
    let stream = async_stream::stream! {
        let initial=serde_json::to_string(&*subscription.initial).unwrap_or_default();
        log::debug!("SSE /api/system/events event=snapshot");
        yield Ok::<_,Infallible>(Event::default().event("snapshot").data(initial));
        loop{if s.system.stopped(){break;}
            match tokio::time::timeout(Duration::from_secs(1),subscription.receiver.recv()).await {
                Ok(Ok(message)) => {
                    let (event, data) = match message {
                        SystemEvent::Snapshot(data) => ("snapshot", serde_json::to_string(&*data).unwrap()),
                        SystemEvent::Metrics(data) => ("metrics", data.to_string()),
                        SystemEvent::Activity(data) => ("activity", data.to_string()),
                    };
                    log::debug!("SSE /api/system/events event={event}");
                    yield Ok(Event::default().event(event).data(data));
                },
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(n))) => {
                    log::debug!("SSE /api/system/events event=gap dropped_frames={n}");
                    yield Ok(Event::default().event("gap").data(json!({"dropped_frames":n}).to_string()));
                    let snapshot = serde_json::to_string(&*s.system.snapshot()).unwrap_or_default();
                    log::debug!("SSE /api/system/events event=snapshot");
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
