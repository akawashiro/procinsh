use super::{
    AppState, api,
    middleware::{guard_http, log_request},
    web,
};
use axum::{Router, middleware};
use std::{net::SocketAddr, sync::Arc};
use tower_http::compression::{
    CompressionLayer, CompressionLevel,
    predicate::{NotForContentType, Predicate, SizeAbove},
};

pub(super) fn router(state: Arc<AppState>, address: SocketAddr) -> Router {
    web::router()
        .merge(api::router())
        // Unknown-length streams (including SSE) are compressed too. Keep the
        // size/content-type exclusions without the default SSE exclusion.
        .layer(compression_layer())
        .layer(middleware::from_fn(move |request, next| {
            guard_http(request, next, address)
        }))
        .layer(middleware::from_fn(log_request))
        .with_state(state)
}

fn compression_layer() -> CompressionLayer<impl Predicate> {
    // Compression runs on the HTTP runtime. Favor activity event latency when
    // compressing the multi-megabyte snapshots on that same runtime.
    CompressionLayer::new()
        .quality(CompressionLevel::Fastest)
        .compress_when(
            SizeAbove::new(256)
                .and(NotForContentType::GRPC)
                .and(NotForContentType::IMAGES),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, HttpBody},
        http::Request,
        response::{Sse, sse::Event},
        routing::get,
    };
    use std::{convert::Infallible, io::Write, time::Duration};
    use tower::ServiceExt;

    #[tokio::test]
    async fn compression_flushes_small_snapshot_activity_and_gap_events() {
        let advance = Arc::new(tokio::sync::Semaphore::new(0));
        let producer = advance.clone();
        let app = Router::new()
            .route(
                "/events",
                get(move || {
                    let producer = producer.clone();
                    async move {
                        Sse::new(async_stream::stream! {
                            for name in ["snapshot", "activity", "gap"] {
                                tokio::time::sleep(Duration::from_millis(100)).await;
                                yield Ok::<_, Infallible>(Event::default().event(name).data("{}"));
                                // Do not generate another event until the client
                                // has received this one through gzip.
                                producer.acquire().await.unwrap().forget();
                            }
                            std::future::pending::<()>().await;
                        })
                    }
                }),
            )
            .layer(compression_layer());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/events")
                    .header("accept-encoding", "gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.headers()["content-encoding"], "gzip");
        let mut body = response.into_body();
        let mut decoder = flate2::write::GzDecoder::new(Vec::new());
        for name in ["snapshot", "activity", "gap"] {
            // Each tiny event must arrive while the producer is idle, even
            // though the stream never ends and never fills an encoder buffer.
            tokio::time::timeout(Duration::from_millis(500), async {
                while !std::str::from_utf8(decoder.get_ref())
                    .unwrap()
                    .contains(&format!("event: {name}\n"))
                {
                    let frame =
                        std::future::poll_fn(|cx| std::pin::Pin::new(&mut body).poll_frame(cx))
                            .await
                            .unwrap()
                            .unwrap()
                            .into_data()
                            .unwrap();
                    decoder.write_all(&frame).unwrap();
                    decoder.flush().unwrap();
                }
            })
            .await
            .expect("SSE compression must flush each event without batching");
            advance.add_permits(1);
        }
    }
}
