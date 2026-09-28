mod api;
mod process;
mod system_monitoring;
mod web;
use anyhow::{Context, Result};
use axum::{
    Json, Router,
    extract::Request,
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use serde_json::json;
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

struct AppState {
    system: Arc<system_monitoring::System>,
    discovery: Mutex<process::Discovery>,
    monitoring: Arc<process::Monitoring>,
    snapshotter: process::Snapshotter,
    interval: Duration,
}
impl AppState {
    fn new(interval: Duration) -> Self {
        Self {
            system: Arc::new(system_monitoring::System::default()),
            discovery: Mutex::new(process::Discovery::default()),
            monitoring: Arc::new(process::Monitoring::new(interval)),
            snapshotter: process::Snapshotter::default(),
            interval,
        }
    }
    fn stop(&self) {
        self.monitoring.stop();
        self.system.stop();
    }
    fn join_collectors(&self) -> Result<()> {
        let process_result = self.monitoring.join_collectors();
        let system_result = self.system.join_workers();
        process_result.and(system_result)
    }
}
pub(super) async fn run(listen: SocketAddr, interval: Duration) -> Result<()> {
    let state = Arc::new(AppState::new(interval));
    let listener = tokio::net::TcpListener::bind(listen)
        .await
        .context("could not bind HTTP listener")?;
    let address = listener.local_addr()?;
    if !address.ip().is_loopback() {
        log::warn!(
            "Warning: remote access exposes process memory and environment variables without authentication or TLS. Use only on a trusted network."
        );
    }
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .context("could not install SIGTERM handler")?;
    let shutdown_state = state.clone();
    log::info!(
        "procinsh {} listening on http://{address} interval={:?}",
        env!("CARGO_PKG_VERSION"),
        interval
    );
    let result = axum::serve(listener, router(state.clone(), address))
        .with_graceful_shutdown(async move {
            tokio::select! {
                result = tokio::signal::ctrl_c() => {
                    match result {
                        Ok(()) => log::info!("received SIGINT; shutting down"),
                        Err(error) => log::error!("SIGINT handler failed: {error}"),
                    }
                },
                _ = terminate.recv() => log::info!("received SIGTERM; shutting down"),
            }
            shutdown_state.stop();
        })
        .await;
    state.stop();
    state.join_collectors()?;
    log::info!("procinsh stopped");
    result.context("HTTP server failed")
}

fn router(state: Arc<AppState>, address: SocketAddr) -> Router {
    web::router()
        .merge(api::router())
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
            ("127.0.0.1:8080", None, "/list", 200),
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

#[cfg(test)]
mod process_api_tests;
#[cfg(test)]
mod system_api_tests;

#[cfg(test)]
mod test_support;
