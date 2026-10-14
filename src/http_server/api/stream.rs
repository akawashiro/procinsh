use crate::http_server::{
    AppState,
    privsep::{MonitorEvent, ObservationSession},
    system::SnapshotEncoder,
};
use axum::response::{
    IntoResponse, Response, Sse,
    sse::{Event, KeepAlive},
};
use std::{
    convert::Infallible,
    sync::Arc,
    time::{Duration, Instant},
};

// HTTP formatting, JSON output, delta selection and SSE recovery all happen
// in the unprivileged worker. IPC carries complete structured observations.
fn encode(event: MonitorEvent, delivery: &mut SnapshotEncoder) -> Vec<(&'static str, String)> {
    match event {
        MonitorEvent::Observation(mut data) => {
            if let Some(samples) = data["live_samples"].as_array_mut() {
                for sample in samples {
                    if let Some(code) = sample.get_mut("disassembly").filter(|v| !v.is_null())
                        && let Ok(mut decoded) = serde_json::from_value::<
                            crate::http_server::process::Disassembly,
                        >(code.clone())
                    {
                        decoded.decode();
                        *code = serde_json::to_value(decoded).unwrap();
                    }
                }
            }
            vec![("observation", data.to_string())]
        }
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
            if let Some(event) = event.take() {
                for (name, data) in encode(event, &mut delivery) {
                    let path = if name == "observation" { "/api/processes/events" } else { "/api/system/events" };
                    log::debug!("SSE {path} event={name} payload_bytes={}", data.len());
                    yield Ok::<_, Infallible>(Event::default().event(name).data(data));
                }
            }
            if closed || state.stopped() { break; }
            tokio::time::sleep(Duration::from_millis(100)).await;
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_instruction_bytes_in_http_observations() {
        let observation = serde_json::json!({"live_samples":[{"disassembly":{
            "address":"0x0000000000001000","bytes":[144,195],"instructions":[],"error":null
        }}]});
        let output = encode(
            MonitorEvent::Observation(observation),
            &mut SnapshotEncoder::default(),
        );
        let payload: serde_json::Value = serde_json::from_str(&output[0].1).unwrap();
        let instructions = payload["live_samples"][0]["disassembly"]["instructions"]
            .as_array()
            .unwrap();
        assert_eq!(instructions.len(), 2);
        assert_eq!(instructions[0]["text"], "nop");
        assert_eq!(instructions[1]["address"], "0x0000000000001001");
    }
}
