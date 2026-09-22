use crate::{
    process::{self, ProcessId},
    state::AppState,
};
use axum::{
    Json, Router,
    extract::{Query, Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{
        Html, IntoResponse, Response, Sse,
        sse::{Event, KeepAlive},
    },
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{convert::Infallible, net::SocketAddr, sync::Arc, time::Duration};

struct ApiError(StatusCode, String);
impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        log::debug!("API error status={} detail={}", self.0, self.1);
        (self.0, Json(json!({"error":self.1}))).into_response()
    }
}
impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        let message = format!("{e:#}");
        let status = if message.contains("Server is stopping") {
            StatusCode::SERVICE_UNAVAILABLE
        } else if message.contains("Process exited") {
            StatusCode::GONE
        } else {
            StatusCode::UNPROCESSABLE_ENTITY
        };
        Self(status, message)
    }
}
type ApiResult = Result<Json<Value>, ApiError>;

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, ApiError> + Send + 'static,
) -> Result<T, ApiError> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
}

pub fn router(state: Arc<AppState>, address: SocketAddr) -> Router {
    Router::new()
        .route(
            "/",
            get(|| async { Html(include_str!("../web/index.html")) }),
        )
        .route(
            "/process/{pid}",
            get(|| async { Html(include_str!("../web/index.html")) }),
        )
        .route(
            "/app.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../../dist/web/app.js"),
                )
            }),
        )
        .route(
            "/style.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    include_str!("../web/style.css"),
                )
            }),
        )
        .route(
            "/space",
            get(|| async { Html(include_str!("../web/space.html")) }),
        )
        .route(
            "/space.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../../dist/web/space.js"),
                )
            }),
        )
        .route(
            "/space-model.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../../dist/web/space-model.js"),
                )
            }),
        )
        .route(
            "/space.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
                    include_str!("../web/space.css"),
                )
            }),
        )
        .route(
            "/vendor/three.module.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../web/vendor/three.module.js"),
                )
            }),
        )
        .route(
            "/vendor/three.core.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../web/vendor/three.core.js"),
                )
            }),
        )
        .route(
            "/vendor/OrbitControls.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
                    include_str!("../web/vendor/OrbitControls.js"),
                )
            }),
        )
        .route("/api/system/status", get(crate::system::http::status))
        .route("/api/system/topology", get(crate::system::http::snapshot))
        .route("/api/system/events", get(crate::system::http::events))
        .route("/api/config", get(config))
        .route("/api/processes", get(processes))
        .route("/api/processes/observation", get(stats))
        .route("/api/processes/threads", get(threads))
        .route("/api/processes/maps", get(maps))
        .route("/api/processes/memory", get(memory))
        .route("/api/processes/environment", get(environment))
        .route("/api/processes/auxv", get(auxv))
        .route("/api/processes/fds", get(fds))
        .route("/api/processes/signals", get(signals))
        .route("/api/processes/snapshot", post(snapshot))
        .route("/api/processes/events", get(events))
        .layer(middleware::from_fn(move |request, next| {
            guard_http(request, next, address)
        }))
        .layer(middleware::from_fn(log_request))
        .with_state(state)
}

async fn log_request(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let path = request.uri().path().to_owned();
    let started = std::time::Instant::now();
    let response = next.run(request).await;
    let status = response.status();
    if status.is_server_error() {
        log::error!(
            "HTTP {method} {path:?} status={} elapsed_ms={}",
            status.as_u16(),
            started.elapsed().as_millis()
        );
    } else {
        log::debug!(
            "HTTP {method} {path:?} status={} elapsed_ms={}",
            status.as_u16(),
            started.elapsed().as_millis()
        );
    }
    response
}

async fn guard_http(request: Request, next: Next, address: SocketAddr) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let authority = host.parse::<axum::http::uri::Authority>().ok();
    let valid_host = authority.as_ref().is_some_and(|a| {
        let port = a.port_u16().unwrap_or(80);
        let name = a.host().trim_matches(['[', ']']);
        port == address.port()
            && if address.ip().is_unspecified() {
                // Explicit remote binding permits numeric host addresses only (no DNS rebinding).
                name.parse::<std::net::IpAddr>().is_ok() || name == "localhost"
            } else {
                name == address.ip().to_string()
                    || address.ip().is_loopback() && name == "localhost"
            }
    });
    let origin_ok = request
        .headers()
        .get(header::ORIGIN)
        .is_none_or(|origin| origin.to_str().is_ok_and(|o| o == format!("http://{host}")));
    let fetch_ok = request
        .headers()
        .get("sec-fetch-site")
        .is_none_or(|v| v == "same-origin" || v == "none");
    if !valid_host || !origin_ok || !fetch_ok {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({"error":"Only requests from this inspector's origin are accepted"})),
        )
            .into_response();
    }
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    headers.insert(header::CONTENT_SECURITY_POLICY, "default-src 'self'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'".parse().unwrap());
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
    headers.insert(header::REFERRER_POLICY, "no-referrer".parse().unwrap());
    response
}

async fn config(State(s): State<Arc<AppState>>) -> Json<Value> {
    Json(
        json!({"version":env!("CARGO_PKG_VERSION"),"interval_ms":s.interval.as_millis(),"history_seconds":60}),
    )
}
async fn processes(State(s): State<Arc<AppState>>) -> ApiResult {
    blocking(move || Ok(Json(json!(s.discovery.lock().unwrap().collect()?)))).await
}
async fn stats(Query(id): Query<ProcessId>) -> ApiResult {
    blocking(move || Ok(Json(json!(crate::state::observation(id, None)?)))).await
}
async fn threads(Query(id): Query<ProcessId>) -> ApiResult {
    blocking(move || Ok(Json(json!(crate::state::observation(id, None)?.threads)))).await
}
async fn maps(Query(id): Query<ProcessId>) -> ApiResult {
    blocking(move || {
        process::check_identity(id)?;
        let result = process::maps::read(id.pid, true);
        let rollup = process::maps::rollup(id.pid);
        process::check_identity(id)?;
        let (maps, error, captured_at) = match result {
            Ok(maps) => (maps, None, Some(process::timestamp_ms())),
            Err(e) => (Vec::new(), Some(process::permission_help("memory maps", e)), None),
        };
        Ok(Json(json!({"process_id":id,"maps":maps,"error":error,"captured_at":captured_at,"rollup":rollup})))
    }).await
}

async fn environment(Query(id): Query<ProcessId>) -> ApiResult {
    blocking(move || {
        process::check_identity(id)?;
        Ok(Json(json!(process::details::environment(id)?)))
    })
    .await
}
async fn fds(Query(id): Query<ProcessId>) -> ApiResult {
    blocking(move || {
        process::check_identity(id)?;
        Ok(Json(json!(process::fds::read(id)?)))
    })
    .await
}
async fn signals(Query(id): Query<ProcessId>) -> ApiResult {
    blocking(move || {
        process::check_identity(id)?;
        Ok(Json(json!(process::signals::read(id)?)))
    })
    .await
}
async fn auxv(Query(id): Query<ProcessId>) -> ApiResult {
    blocking(move || {
        process::check_identity(id)?;
        Ok(Json(json!(process::details::auxv(id)?)))
    })
    .await
}

#[derive(Deserialize)]
struct MemoryQuery {
    pid: i32,
    start_time_ticks: u64,
    address: String,
    #[serde(default = "default_length")]
    length: usize,
}
fn default_length() -> usize {
    256
}
async fn memory(Query(q): Query<MemoryQuery>) -> ApiResult {
    let address = if let Some(hex) = q
        .address
        .strip_prefix("0x")
        .or_else(|| q.address.strip_prefix("0X"))
    {
        u64::from_str_radix(hex, 16)
    } else {
        q.address.parse()
    }
    .map_err(|_| {
        ApiError(
            StatusCode::BAD_REQUEST,
            "Invalid address; use 0x hexadecimal or decimal".into(),
        )
    })?;
    if !(1..=process::memory::MAX_READ).contains(&q.length)
        || address.checked_add(q.length as u64).is_none()
    {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "Invalid memory range; length must be 1..=65536".into(),
        ));
    }
    blocking(move || {
        let id = ProcessId {
            pid: q.pid,
            start_time_ticks: q.start_time_ticks,
        };
        process::check_identity(id)?;
        Ok(Json(json!(process::memory::read(id, address, q.length)?)))
    })
    .await
}
async fn snapshot(State(s): State<Arc<AppState>>, Json(id): Json<ProcessId>) -> ApiResult {
    blocking(move || {
        let _snapshot = s.snapshot_lock.lock().unwrap();
        process::check_identity(id)?;
        Ok(Json(json!(crate::snapshot::capture(
            id,
            s.symbols.clone()
        )?)))
    })
    .await
}
async fn events(
    State(s): State<Arc<AppState>>,
    Query(id): Query<ProcessId>,
) -> Result<Response, ApiError> {
    let permit = s.reserve().map_err(|code| {
        ApiError(
            code,
            if code == StatusCode::TOO_MANY_REQUESTS {
                "Too many viewers"
            } else {
                "Server is stopping"
            }
            .into(),
        )
    })?;
    let state = s.clone();
    let mut session = blocking(move || Ok(state.observe(id, permit)?)).await?;
    let initial = session.receiver.borrow_and_update().clone();
    let stream = async_stream::stream! {
        log::debug!("SSE /api/processes/events event=observation pid={} start_time_ticks={}", id.pid, id.start_time_ticks);
        yield Ok::<_, Infallible>(Event::default().event("observation").json_data(initial).unwrap());
        loop {
            if s.is_stopped() { break; }
            let received = tokio::select! {
                received = session.receiver.changed() => received,
                _ = tokio::time::sleep(Duration::from_millis(100)) => { continue; }
            };
            if received.is_err() { break; }
            let data = session.receiver.borrow_and_update().clone();
            let exited = data.exited;
            log::debug!("SSE /api/processes/events event=observation pid={} start_time_ticks={} exited={exited}", id.pid, id.start_time_ticks);
            yield Ok(Event::default().event("observation").json_data(data).unwrap());
            if exited { break; }
        }
        // Retain the permit for the complete stream lifetime, not just initialization.
        drop(session);
    };
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use tower::ServiceExt;
    fn app() -> Router {
        router(
            Arc::new(AppState::new(Duration::from_secs(1))),
            "127.0.0.1:8080".parse().unwrap(),
        )
    }
    #[tokio::test]
    async fn security_and_embedded_resources() {
        for (host, origin, path, status) in [
            ("evil.test:8080", None, "/api/processes", 403),
            (
                "127.0.0.1:8080",
                Some("https://evil.test"),
                "/api/processes",
                403,
            ),
            ("127.0.0.1:8080", None, "/", 200),
            ("localhost:8080", None, "/app.js", 200),
        ] {
            let mut req = Request::builder().uri(path).header("host", host);
            if let Some(origin) = origin {
                req = req.header("origin", origin);
            }
            let response = app()
                .oneshot(req.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), status);
        }
    }
}
