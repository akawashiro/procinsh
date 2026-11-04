use super::{
    AppState,
    hosts::{self, HostInfo, HostRegistry},
    router::router,
};
use anyhow::{Context, Result, ensure};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{sync::watch, task::JoinSet};

/// Listeners and the links to other hosts.
pub(crate) struct ServerConfig {
    pub(crate) listen: Vec<SocketAddr>,
    /// Addresses on which this hub accepts agents.
    pub(crate) accept_agents: Vec<SocketAddr>,
    /// The hub address this agent links to.
    pub(crate) connect: Option<String>,
    /// The name shown by a hub; the kernel host name by default.
    pub(crate) host_name: Option<String>,
    pub(crate) agent_token: Option<String>,
    /// Privacy mode's logblur level (1–4), or `None` when it is off.
    pub(crate) privacy_level: Option<u8>,
}

/// Bind every address before serving, then share collectors across listeners.
/// Each listener validates requests against its own bound address. A shutdown
/// signal or a listener failure stops all listeners and joins the collectors.
pub(super) async fn run(config: ServerConfig, client: super::client::MonitorClient) -> Result<()> {
    let listen = &config.listen;
    ensure!(!listen.is_empty(), "no HTTP listen addresses specified");
    let mut listeners = Vec::with_capacity(listen.len());
    for address in listen {
        let listener = tokio::net::TcpListener::bind(address)
            .await
            .with_context(|| format!("could not bind HTTP listener on {address}"))?;
        let address = listener.local_addr()?;
        listeners.push((listener, address));
    }
    let mut agent_listeners = Vec::with_capacity(config.accept_agents.len());
    for address in &config.accept_agents {
        let listener = tokio::net::TcpListener::bind(address)
            .await
            .with_context(|| format!("could not bind agent listener on {address}"))?;
        agent_listeners.push(listener);
    }
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .context("could not install SIGTERM handler")?;
    let interval = Duration::from_secs(1);
    let info = HostInfo::local(config.host_name);
    let registry = Arc::new(HostRegistry::new(info.clone()));
    let state = Arc::new(AppState::remote(interval, client.clone(), registry.clone()));
    let (shutdown, _) = watch::channel(false);
    let token: Option<Arc<str>> = config.agent_token.map(Into::into);
    let mut links = JoinSet::new();
    for listener in agent_listeners {
        log::info!(
            "accepting agents on {} as host {}",
            listener.local_addr()?,
            info.name
        );
        links.spawn(hosts::run_agent_listener(
            listener,
            registry.clone(),
            token.clone(),
            shutdown.subscribe(),
        ));
    }
    if let Some(address) = config.connect {
        log::info!("linking to hub {address} as host {}", info.name);
        links.spawn(hosts::run_hub_connector(
            address,
            info,
            token,
            client,
            shutdown.subscribe(),
        ));
    }
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
    while links.join_next().await.is_some() {}
    while let Some(server) = servers.join_next().await {
        result = result.and(match server {
            Ok(result) => result.context("HTTP server failed"),
            Err(error) => Err(error).context("HTTP server task failed"),
        });
    }
    log::info!("procinsh stopped");
    result
}
