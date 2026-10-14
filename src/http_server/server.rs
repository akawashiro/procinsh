use super::{AppState, privsep::MonitorClient, router::router};
use anyhow::{Context, Result, ensure};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{sync::watch, task::JoinSet};

/// Bind every address before serving and share one monitor client across
/// listeners. Each listener validates requests against its bound address.
/// Shutdown drains HTTP streams; the parent joins collectors after disconnect.
pub(crate) async fn run(listen: &[SocketAddr], client: MonitorClient) -> Result<()> {
    ensure!(!listen.is_empty(), "no HTTP listen addresses specified");
    let mut listeners = Vec::with_capacity(listen.len());
    for address in listen {
        let listener = tokio::net::TcpListener::bind(address)
            .await
            .with_context(|| format!("could not bind HTTP listener on {address}"))?;
        let address = listener.local_addr()?;
        listeners.push((listener, address));
    }
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .context("could not install SIGTERM handler")?;
    let interval = Duration::from_secs(1);
    let state = Arc::new(AppState::remote(interval, client));
    let (shutdown, _) = watch::channel(false);
    let mut servers = JoinSet::new();
    for (listener, address) in listeners {
        if !address.ip().is_loopback() {
            log::warn!(
                "Warning: remote access exposes process memory and environment variables without authentication or TLS. Use only on a trusted network."
            );
        }
        let app = router(state.clone(), address);
        let mut stop = shutdown.subscribe();
        servers.spawn(async move {
            axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    let _ = stop.changed().await;
                })
                .await
        });
        log::info!(
            "procinsh {} listening on http://{address} interval={:?}",
            env!("CARGO_PKG_VERSION"),
            interval
        );
    }
    let mut result = tokio::select! {
        result = tokio::signal::ctrl_c() => {
            match &result {
                Ok(()) => log::info!("received SIGINT; shutting down"),
                Err(error) => log::error!("SIGINT handler failed: {error}"),
            }
            result.context("SIGINT handler failed")
        },
        _ = terminate.recv() => {
            log::info!("received SIGTERM; shutting down");
            Ok(())
        },
        result = servers.join_next() => {
            match result {
                Some(Ok(result)) => result.context("HTTP server failed"),
                Some(Err(error)) => Err(error).context("HTTP server task failed"),
                None => Err(anyhow::anyhow!("no HTTP listeners")),
            }
        },
    };
    state.stop();
    shutdown.send_replace(true);
    while let Some(server) = servers.join_next().await {
        result = result.and(match server {
            Ok(result) => result.context("HTTP server failed"),
            Err(error) => Err(error).context("HTTP server task failed"),
        });
    }

    log::info!("procinsh stopped");
    result
}
