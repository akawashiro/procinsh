use super::{AppState, router::router};
use axum::{Router, extract::Request};
use std::{sync::Arc, time::Duration};

use axum::body::Body;
use tower::ServiceExt;

fn app() -> Router {
    router(
        Arc::new(AppState::new(Duration::from_secs(1))),
        "127.0.0.1:8080".parse().unwrap(),
    )
}

#[tokio::test]
async fn headers_report_the_binary_revision_on_all_pages() {
    let sha = env!("PROCINSH_BUILD_GIT_SHA");
    let dirty = env!("PROCINSH_BUILD_GIT_DIRTY") == "true";
    for path in ["/list", "/process/1", "/space"] {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("host", "localhost:8080")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains(&format!("procinsh v{}", env!("CARGO_PKG_VERSION"))));
        if sha.is_empty() {
            assert!(!html.contains("id=\"build-commit\""));
        } else {
            assert!(html.contains(&format!("/commit/{sha}")));
            let suffix = if dirty { "-dirty" } else { "" };
            assert!(html.contains(&format!(">{}{suffix}</a>", &sha[..7])));
        }
    }
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
        ("localhost:8080", None, "/THREE-LICENSE.txt", 200),
        ("localhost:8080", None, "/assets/missing.js", 404),
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

#[tokio::test]
async fn static_assets_support_head_and_preserve_unknown_method_responses() {
    for (method, path, status) in [
        ("HEAD", "/THREE-LICENSE.txt", 200),
        ("POST", "/THREE-LICENSE.txt", 405),
        ("POST", "/api/missing", 404),
        ("GET", "/assets/missing.js", 404),
    ] {
        let response = app()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("host", "localhost:8080")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status().as_u16(), status, "{method} {path}");
        if method == "HEAD" {
            assert_eq!(
                response.headers()["content-type"],
                "text/plain; charset=utf-8"
            );
            let body = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            assert!(body.is_empty());
        }
    }
}

#[tokio::test]
async fn gzip_negotiation_and_resources() {
    use std::io::Read;
    let mut paths = std::collections::BTreeSet::from([
        "/".to_owned(),
        "/space".to_owned(),
        "/api/processes".to_owned(),
    ]);
    for page in ["/list", "/process/1", "/space"] {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri(page)
                    .header("host", "localhost:8080")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        for attribute in ["src=\"", "href=\""] {
            for value in html.split(attribute).skip(1) {
                let path = value.split('"').next().unwrap();
                if path.starts_with("/assets/") {
                    paths.insert(path.to_owned());
                }
            }
        }
    }
    for path in paths {
        let request = |encoding: &str| {
            Request::builder()
                .uri(&path)
                .header("host", "localhost:8080")
                .header("accept-encoding", encoding)
                .body(Body::empty())
                .unwrap()
        };
        let plain = app().oneshot(request("identity")).await.unwrap();
        assert_eq!(plain.status(), 200);
        assert!(!plain.headers().contains_key("content-encoding"));
        let plain = axum::body::to_bytes(plain.into_body(), usize::MAX)
            .await
            .unwrap();
        let gzip = app().oneshot(request("gzip")).await.unwrap();
        if plain.len() <= 256 {
            assert!(!gzip.headers().contains_key("content-encoding"));
            let body = axum::body::to_bytes(gzip.into_body(), usize::MAX)
                .await
                .unwrap();
            assert_eq!(body, plain);
            continue;
        }
        assert_eq!(gzip.headers()["content-encoding"], "gzip");
        assert!(
            gzip.headers()["vary"]
                .to_str()
                .unwrap()
                .contains("accept-encoding")
        );
        let compressed = axum::body::to_bytes(gzip.into_body(), usize::MAX)
            .await
            .unwrap();
        let mut decoded = Vec::new();
        flate2::read::GzDecoder::new(&compressed[..])
            .read_to_end(&mut decoded)
            .unwrap();
        // The live process list can change between requests.
        if path == "/api/processes" {
            serde_json::from_slice::<serde_json::Value>(&decoded).unwrap();
        } else {
            assert_eq!(decoded, plain);
        }
    }
    for encoding in ["identity", "gzip;q=0", "br"] {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header("host", "localhost:8080")
                    .header("accept-encoding", encoding)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(!response.headers().contains_key("content-encoding"));
    }
    for path in ["/missing", "/api/config"] {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri(path)
                    .header("host", "localhost:8080")
                    .header("accept-encoding", "gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(!response.headers().contains_key("content-encoding"));
    }
}

#[tokio::test]
async fn gzip_sse_flushes_before_stream_ends() {
    use axum::body::HttpBody;
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/api/system/events")
                .header("host", "localhost:8080")
                .header("accept-encoding", "gzip")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers()["content-encoding"], "gzip");
    let mut body = response.into_body();
    use std::io::Write;
    let mut decoder = flate2::write::GzDecoder::new(Vec::new());
    let started = std::time::Instant::now();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let frame = std::future::poll_fn(|cx| std::pin::Pin::new(&mut body).poll_frame(cx))
                .await
                .unwrap()
                .unwrap()
                .into_data()
                .unwrap();
            decoder.write_all(&frame).unwrap();
            decoder.flush().unwrap();
            let output = std::str::from_utf8(decoder.get_ref()).unwrap();
            if output.contains("event: snapshot\n") {
                assert!(started.elapsed() < Duration::from_secs(2));
            }
            if output.contains("event: activity\n") {
                break;
            }
        }
    })
    .await
    .expect("gzip must flush an open SSE stream");
}
