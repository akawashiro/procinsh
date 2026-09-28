use axum::{
    Json,
    extract::Request,
    http::{StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde_json::json;
use std::net::SocketAddr;

pub(super) async fn log_request(request: Request, next: Next) -> Response {
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

pub(super) async fn guard_http(request: Request, next: Next, address: SocketAddr) -> Response {
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
