//! The hub side of agent links.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`PING_INTERVAL`] | `pub(super)` | `const PING_INTERVAL: Duration` | //! | [`IDLE_TIMEOUT`] | `pub(super)` | `const IDLE_TIMEOUT: Duration` | //! | [`run_agent_listener`] | `pub(in crate::http_server)` | `async fn run_agent_listener( listener: `[`TcpListener`]`, registry: `[`Arc`]`<`[`HostRegistry`]`>, token: `[`Option`]`<`[`Arc`]`<str>>, mut shutdown: `[`watch::Receiver`]`<bool>, )` | use super::identity::{HostInfo, valid_host_name}` | //! | [`IDLE_TIMEOUT`] | `pub(super)` | `const IDLE_TIMEOUT: Duration` | //! | [`run_agent_listener`] | `pub(in crate::http_server)` | `async fn run_agent_listener( listener: `[`TcpListener`]`, registry: `[`Arc`]`<`[`HostRegistry`]`>, token: `[`Option`]`<`[`Arc`]`<str>>, mut shutdown: `[`watch::Receiver`]`<bool>, )` | use super::identity::{HostInfo, valid_host_name}` | //! | [`run_agent_listener`] | `pub(in crate::http_server)` | `async fn run_agent_listener( listener: `[`TcpListener`]`, registry: `[`Arc`]`<`[`HostRegistry`]`>, token: `[`Option`]`<`[`Arc`]`<str>>, mut shutdown: `[`watch::Receiver`]`<bool>, )` | use super::identity::{HostInfo, valid_host_name}` | //! | [`IDLE_TIMEOUT`] | `pub(super)` | `const IDLE_TIMEOUT: Duration` | //! | [`run_agent_listener`] | `pub(in crate::http_server)` | `async fn run_agent_listener( listener: `[`TcpListener`]`, registry: `[`Arc`]`<`[`HostRegistry`]`>, token: `[`Option`]`<`[`Arc`]`<str>>, mut shutdown: `[`watch::Receiver`]`<bool>, )` | use super::identity::{HostInfo, valid_host_name}` | //! | [`IDLE_TIMEOUT`] | `pub(super)` | `const IDLE_TIMEOUT: Duration` | //! | [`run_agent_listener`] | `pub(in crate::http_server)` | `async fn run_agent_listener( listener: `[`TcpListener`]`, registry: `[`Arc`]`<`[`HostRegistry`]`>, token: `[`Option`]`<`[`Arc`]`<str>>, mut shutdown: `[`watch::Receiver`]`<bool>, )` | use super::identity::{HostInfo, valid_host_name}` | //! | [`run_agent_listener`] | `pub(in crate::http_server)` | `async fn run_agent_listener( listener: `[`TcpListener`]`, registry: `[`Arc`]`<`[`HostRegistry`]`>, token: `[`Option`]`<`[`Arc`]`<str>>, mut shutdown: `[`watch::Receiver`]`<bool>, )` | use super::identity::{HostInfo, valid_host_name}` | //! | [`run_agent_listener`] | `pub(in crate::http_server)` | `async fn run_agent_listener( listener: `[`TcpListener`]`, registry: `[`Arc`]`<`[`HostRegistry`]`>, token: `[`Option`]`<`[`Arc`]`<str>>, mut shutdown: `[`watch::Receiver`]`<bool>, )` | use super::identity::{HostInfo, valid_host_name}` |
//! | [`IDLE_TIMEOUT`] | `pub(super)` | `const IDLE_TIMEOUT: Duration` | //! | [`run_agent_listener`] | `pub(in crate::http_server)` | `async fn run_agent_listener( listener: `[`TcpListener`]`, registry: `[`Arc`]`<`[`HostRegistry`]`>, token: `[`Option`]`<`[`Arc`]`<str>>, mut shutdown: `[`watch::Receiver`]`<bool>, )` | use super::identity::{HostInfo, valid_host_name}` | //! | [`IDLE_TIMEOUT`] | `pub(super)` | `const IDLE_TIMEOUT: Duration` | //! | [`run_agent_listener`] | `pub(in crate::http_server)` | `async fn run_agent_listener( listener: `[`TcpListener`]`, registry: `[`Arc`]`<`[`HostRegistry`]`>, token: `[`Option`]`<`[`Arc`]`<str>>, mut shutdown: `[`watch::Receiver`]`<bool>, )` | use super::identity::{HostInfo, valid_host_name}` | //! | [`run_agent_listener`] | `pub(in crate::http_server)` | `async fn run_agent_listener( listener: `[`TcpListener`]`, registry: `[`Arc`]`<`[`HostRegistry`]`>, token: `[`Option`]`<`[`Arc`]`<str>>, mut shutdown: `[`watch::Receiver`]`<bool>, )` | use super::identity::{HostInfo, valid_host_name}` | //! | [`IDLE_TIMEOUT`] | `pub(super)` | `const IDLE_TIMEOUT: Duration` | //! | [`run_agent_listener`] | `pub(in crate::http_server)` | `async fn run_agent_listener( listener: `[`TcpListener`]`, registry: `[`Arc`]`<`[`HostRegistry`]`>, token: `[`Option`]`<`[`Arc`]`<str>>, mut shutdown: `[`watch::Receiver`]`<bool>, )` | use super::identity::{HostInfo, valid_host_name}` | //! | [`IDLE_TIMEOUT`] | `pub(super)` | `const IDLE_TIMEOUT: Duration` | //! | [`run_agent_listener`] | `pub(in crate::http_server)` | `async fn run_agent_listener( listener: `[`TcpListener`]`, registry: `[`Arc`]`<`[`HostRegistry`]`>, token: `[`Option`]`<`[`Arc`]`<str>>, mut shutdown: `[`watch::Receiver`]`<bool>, )` | use super::identity::{HostInfo, valid_host_name}` | //! | [`run_agent_listener`] | `pub(in crate::http_server)` | `async fn run_agent_listener( listener: `[`TcpListener`]`, registry: `[`Arc`]`<`[`HostRegistry`]`>, token: `[`Option`]`<`[`Arc`]`<str>>, mut shutdown: `[`watch::Receiver`]`<bool>, )` | use super::identity::{HostInfo, valid_host_name}` | //! | [`run_agent_listener`] | `pub(in crate::http_server)` | `async fn run_agent_listener( listener: `[`TcpListener`]`, registry: `[`Arc`]`<`[`HostRegistry`]`>, token: `[`Option`]`<`[`Arc`]`<str>>, mut shutdown: `[`watch::Receiver`]`<bool>, )` | use super::identity::{HostInfo, valid_host_name}` |
//! | [`run_agent_listener`] | `pub(in crate::http_server)` | `async fn run_agent_listener( listener: `[`TcpListener`]`, registry: `[`Arc`]`<`[`HostRegistry`]`>, token: `[`Option`]`<`[`Arc`]`<str>>, mut shutdown: `[`watch::Receiver`]`<bool>, )` |

use super::identity::{HostInfo, valid_host_name};
use super::link::AgentLink;
use super::registry::HostRegistry;
use super::wire::{self, Message};
use anyhow::{Context, Result, bail};
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::watch,
    time::{Instant, timeout},
};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
pub(super) const PING_INTERVAL: Duration = Duration::from_secs(10);
/// A link without any message for this long is closed.
pub(super) const IDLE_TIMEOUT: Duration = Duration::from_secs(30);

/// Accepts agent links until `shutdown` changes. `token`, when set, must match each
/// agent's hello.
pub(in crate::http_server) async fn run_agent_listener(
    listener: TcpListener,
    registry: Arc<HostRegistry>,
    token: Option<Arc<str>>,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        let (stream, peer) = tokio::select! {
            accepted = listener.accept() => match accepted {
                Ok(accepted) => accepted,
                Err(error) => {
                    log::warn!("agent accept failed: {error}");
                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            },
            _ = shutdown.changed() => return,
        };
        let (registry, token, shutdown) = (registry.clone(), token.clone(), shutdown.clone());
        tokio::spawn(async move {
            if let Err(error) = serve_agent(stream, peer, &registry, token, shutdown).await {
                log::warn!("agent link from {peer} closed: {error:#}");
            }
        });
    }
}

async fn accept_agent_handshake(
    stream: &mut TcpStream,
    registry: &HostRegistry,
    token: Option<&str>,
    link: Arc<AgentLink>,
) -> Result<(String, u64)> {
    use crate::http_server::hosts::{
        identity::protect_remote_host_info, registry::admit_host_registry,
    };

    let hello = timeout(HANDSHAKE_TIMEOUT, wire::read_host_message(stream))
        .await
        .context("agent hello timed out")??;
    let Message::Hello {
        protocol,
        host,
        machine_id,
        version,
        kernel,
        arch,
        token: offered,
    } = hello
    else {
        bail!("agent did not start with hello");
    };
    let mut info = HostInfo {
        name: host.clone(),
        machine_id,
        version,
        kernel,
        arch,
    };
    let admitted = if protocol != wire::PROTOCOL {
        Err(format!(
            "agent protocol {protocol} is not supported (hub protocol {})",
            wire::PROTOCOL
        ))
    } else if let Some(expected) = token
        && !wire::same_secret(expected, offered.as_deref().unwrap_or_default())
    {
        Err("agent token mismatch".into())
    } else if !valid_host_name(&info.name) {
        Err("invalid agent host name".into())
    } else {
        protect_remote_host_info(&mut info);
        admit_host_registry(registry, info.clone(), link)
    };
    match admitted {
        Ok(id) => {
            wire::write_host_message(
                stream,
                Message::Welcome {
                    protocol: wire::PROTOCOL,
                    version: env!("CARGO_PKG_VERSION").into(),
                },
            )
            .await?;
            Ok((info.name, id))
        }
        Err(reason) => {
            wire::write_host_message(
                stream,
                Message::Reject {
                    reason: reason.clone(),
                },
            )
            .await?;
            bail!("rejected agent: {reason}");
        }
    }
}

async fn serve_agent(
    mut stream: TcpStream,
    peer: SocketAddr,
    registry: &HostRegistry,
    token: Option<Arc<str>>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<()> {
    use crate::http_server::hosts::{
        link::{close_agent_link, create_agent_link},
        registry::{disconnected_host_registry, host_registry_is_current},
    };

    stream.set_nodelay(true)?;
    let (link, outgoing) = create_agent_link();
    let link = Arc::new(link);
    let (name, id) =
        accept_agent_handshake(&mut stream, registry, token.as_deref(), link.clone()).await?;
    log::info!("agent {name} connected from {peer}");
    let result = relay_agent_messages(stream, &link, outgoing, &mut shutdown, || {
        host_registry_is_current(registry, &name, id)
    })
    .await;
    disconnected_host_registry(registry, &name, id);
    close_agent_link(&link);
    log::info!("agent {name} disconnected");
    result
}

/// Moves requests to the agent and its responses back until the link fails,
/// idles, is replaced, or the hub shuts down.
async fn relay_agent_messages(
    stream: TcpStream,
    link: &AgentLink,
    mut outgoing: tokio::sync::mpsc::Receiver<Message>,
    shutdown: &mut watch::Receiver<bool>,
    current: impl Fn() -> bool,
) -> Result<()> {
    use crate::http_server::hosts::link::{complete_agent_link, send_agent_link};

    let (mut reader, mut writer) = stream.into_split();
    // Dedicated tasks keep partially read or written frames intact across select branches.
    let (messages, mut incoming) = tokio::sync::mpsc::channel(8);
    let reading = tokio::spawn(async move {
        loop {
            let message = wire::read_host_message(&mut reader).await;
            let failed = message.is_err();
            if messages.send(message).await.is_err() || failed {
                break;
            }
        }
    });
    let (written, mut write_failed) = tokio::sync::oneshot::channel();
    let writing = tokio::spawn(async move {
        while let Some(message) = outgoing.recv().await {
            if let Err(error) = wire::write_host_message(&mut writer, message).await {
                let _ = written.send(error);
                return;
            }
        }
    });
    let mut ping = tokio::time::interval(PING_INTERVAL);
    let mut last_message = Instant::now();
    let result = loop {
        tokio::select! {
            message = incoming.recv() => {
                let Some(message) = message else { break Ok(()) };
                last_message = Instant::now();
                match message {
                    Ok(Message::Response { id, response }) => complete_agent_link(link, id, response),
                    // A full queue is already traffic; a lost pong only delays idleness.
                    Ok(Message::Ping) => drop(send_agent_link(link, Message::Pong)),
                    Ok(Message::Pong) => {}
                    Ok(_) => break Err(anyhow::anyhow!("unexpected message from agent")),
                    Err(error) => break Err(error),
                }
            }
            error = &mut write_failed => {
                break Err(error.unwrap_or_else(|_| anyhow::anyhow!("agent link writer stopped")));
            }
            _ = ping.tick() => {
                if !current() {
                    break Ok(());
                }
                if last_message.elapsed() > IDLE_TIMEOUT {
                    break Err(anyhow::anyhow!("agent link idle for {IDLE_TIMEOUT:?}"));
                }
                drop(send_agent_link(link, Message::Ping));
            }
            _ = shutdown.changed() => break Ok(()),
        }
    };
    reading.abort();
    writing.abort();
    result
}
