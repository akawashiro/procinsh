use super::{AppState, router::router};
use anyhow::{Context, Result};
use std::{net::SocketAddr, sync::Arc, time::Duration};

pub(crate) async fn run(listen: SocketAddr) -> Result<()> {
    let interval = Duration::from_secs(1);
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
