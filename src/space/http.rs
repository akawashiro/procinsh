use crate::state::AppState;
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{
        IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{convert::Infallible, sync::Arc, time::Duration};
pub async fn status(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(s.space.status.lock().unwrap().clone())
}
pub async fn snapshot(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(json!(**s.space.snapshot.read().unwrap()))
}
pub async fn lease(
    State(s): State<Arc<AppState>>,
    Json(request): Json<super::LeaseRequest>,
) -> Result<Json<super::LeaseResponse>, (StatusCode, Json<Value>)> {
    s.space.lease(request).map(Json).map_err(|e| {
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({"error":e.to_string()})),
        )
    })
}
#[derive(Deserialize)]
pub struct Token {
    token: String,
}
pub async fn release(State(s): State<Arc<AppState>>, Json(request): Json<Token>) -> Json<Value> {
    s.space.release(&request.token);
    Json(json!({"ok":true}))
}
pub async fn events(
    State(s): State<Arc<AppState>>,
    Query(request): Query<Token>,
) -> Result<Response, StatusCode> {
    if !s.space.leases.lock().unwrap().contains_key(&request.token) {
        return Err(StatusCode::FORBIDDEN);
    }
    let mut rx = s.space.events.subscribe();
    let stream = async_stream::stream! {
        let initial=serde_json::to_string(&**s.space.snapshot.read().unwrap()).unwrap_or_default();
        yield Ok::<_,Infallible>(Event::default().event("topology").data(initial));
        loop{if s.space.stopped() || !s.space.leases.lock().unwrap().contains_key(&request.token){break;}
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
