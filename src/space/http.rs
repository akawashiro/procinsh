use crate::state::AppState;
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
};
use serde_json::{Value, json};
use std::{convert::Infallible, sync::Arc, time::Duration};
pub async fn status(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(s.space.status.lock().unwrap().clone())
}
pub async fn snapshot(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(json!(**s.space.snapshot.read().unwrap()))
}
pub async fn events(State(s): State<Arc<AppState>>) -> Result<Response, StatusCode> {
    let viewer = s.space.viewer()?;
    let mut rx = s.space.events.subscribe();
    let stream = async_stream::stream! {
        let _viewer = viewer;
        let initial=serde_json::to_string(&**s.space.snapshot.read().unwrap()).unwrap_or_default();
        yield Ok::<_,Infallible>(Event::default().event("topology").data(initial));
        loop{if s.space.stopped(){break;}
            match tokio::time::timeout(Duration::from_secs(1),rx.recv()).await {
                Ok(Ok(message))=>{if let Ok(value)=serde_json::from_str::<Value>(&message){yield Ok(Event::default().event(value["event"].as_str().unwrap_or("message")).data(value["data"].to_string()));}},
                Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(n)))=>{yield Ok(Event::default().event("gap").data(json!({"dropped_frames":n}).to_string()));let snapshot=serde_json::to_string(&**s.space.snapshot.read().unwrap()).unwrap_or_default(); yield Ok(Event::default().event("topology").data(snapshot));},
                Ok(Err(_))=>break,Err(_)=>{}
            }
        }
    };
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
        .into_response())
}
