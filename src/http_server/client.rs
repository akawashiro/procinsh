//! Queue bounded helper requests and manage local or remote observation sessions.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`MonitorClient`] | `pub(super)` | `struct MonitorClient` |
//! | [`ObservationSession`] | `pub(super)` | `struct ObservationSession` |
//! | [`create_agent_monitor_client`] | `pub(super)` | `fn create_agent_monitor_client(link: `[`Arc`]`<`[`AgentLink`]`>) -> `[`MonitorClient`] |
//! | [`create_unavailable_monitor_client`] | `pub(super)` | `fn create_unavailable_monitor_client() -> `[`MonitorClient`] |
//! | [`create_monitor_client`] | `pub(super)` | `fn create_monitor_client(mut socket: `[`UnixStream`]`) -> `[`Result`]`<`[`MonitorClient`]`>` |
//! | [`request_monitor_client`] | `pub(super)` | `async fn request_monitor_client( state: &`[`MonitorClient`]`, request: `[`MonitorRequest`]`, ) -> `[`Result`]`<`[`MonitorResponse`]`>` |
//! | [`exchange_monitor_client`] | `pub(super)` | `async fn exchange_monitor_client( state: &`[`MonitorClient`]`, request: `[`MonitorRequest`]`, ) -> `[`Result`]`<`[`crate::ipc::schema::MonitorResponse`]`>` |
//! | [`observe_monitor_client`] | `pub(super)` | `async fn observe_monitor_client( state: &`[`MonitorClient`]`, request: `[`MonitorRequest`]`, ) -> `[`Result`]`<`[`ObservationSession`]`>` |
//! | [`close_monitor_client`] | `pub(super)` | `fn close_monitor_client(state: &`[`MonitorClient`]`, session: u64)` |
//! | [`poll_observation_session`] | `pub(super)` | `async fn poll_observation_session( state: &`[`ObservationSession`]`, ) -> `[`Result`]`<(`[`Option`]`<`[`MonitorEvent`]`>, bool)>` |

use super::hosts::AgentLink;
use crate::ipc::transport;
use anyhow::{Context, Result, bail};
use std::{net::Shutdown, os::unix::net::UnixStream, sync::Arc};
use tokio::sync::{mpsc, oneshot};
use {
    super::protocol::{MonitorEvent, MonitorResponse},
    crate::ipc::schema::{MAX_REQUEST, MAX_RESPONSE, MonitorRequest, TIMEOUT, WireResponse},
};

struct PendingRequest {
    request: MonitorRequest,
    reply: oneshot::Sender<WireResponse>,
}
enum Backend {
    Unavailable,
    Remote {
        sender: mpsc::Sender<PendingRequest>,
        shutdown: UnixStream,
        processor: Box<tokio::sync::Mutex<super::processing::ObservationProcessor>>,
        closed_sessions: std::sync::Mutex<Vec<u64>>,
    },
    /// A remote host's helper, reached through its agent link.
    Agent(Arc<AgentLink>),
}

/// A bounded queue and one I/O worker serialize socket access. A timed-out
/// exchange closes the connection, so replies can never match a later request.
#[derive(Clone)]
pub(super) struct MonitorClient(Arc<Backend>);
pub(super) fn create_agent_monitor_client(link: Arc<AgentLink>) -> MonitorClient {
    MonitorClient(Arc::new(Backend::Agent(link)))
}

pub(super) fn create_unavailable_monitor_client() -> MonitorClient {
    MonitorClient(Arc::new(Backend::Unavailable))
}

pub(super) fn create_monitor_client(mut socket: UnixStream) -> Result<MonitorClient> {
    transport::configure_ipc_socket(&socket)?;
    let shutdown = socket.try_clone()?;
    let (sender, mut receiver) = mpsc::channel::<PendingRequest>(64);
    std::thread::Builder::new()
        .name("monitor-ipc".into())
        .spawn(move || {
            while let Some(pending) = receiver.blocking_recv() {
                // Cancellation before dispatch must not create an orphaned
                // collector. Cancellation during dispatch is bounded by leases.
                if pending.reply.is_closed()
                    && !matches!(pending.request, MonitorRequest::Close { .. })
                {
                    continue;
                }
                let response = transport::send(&mut socket, &pending.request, MAX_REQUEST)
                    .and_then(|()| transport::read::<WireResponse>(&mut socket, MAX_RESPONSE));
                match response {
                    Ok(response) => {
                        let _ = pending.reply.send(response);
                    }
                    Err(error) => {
                        let _ = pending
                            .reply
                            .send(Err(format!("monitor IPC unavailable: {error:#}")));
                        let _ = socket.shutdown(Shutdown::Both);
                        break;
                    }
                }
            }
        })?;
    Ok(MonitorClient(Arc::new(Backend::Remote {
        sender,
        shutdown,
        processor: Box::new(tokio::sync::Mutex::new(Default::default())),
        closed_sessions: std::sync::Mutex::new(Vec::new()),
    })))
}

pub(super) async fn request_monitor_client(
    state: &MonitorClient,
    request: MonitorRequest,
) -> Result<MonitorResponse> {
    use crate::http_server::{
        hosts::request_agent_link,
        processing::{close_observation_processor, process_observation_processor},
    };

    if let Backend::Agent(link) = &*state.0 {
        let mut response = request_agent_link(link, request).await?;
        super::privacy::scrub_response(&mut response);
        return Ok(response);
    }
    let session = match &request {
        MonitorRequest::Poll { session } => Some(*session),
        _ => None,
    };
    let mut response =
        super::protocol::convert_monitor_response(exchange_monitor_client(state, request).await?)?;
    if let Backend::Remote {
        processor,
        closed_sessions,
        ..
    } = &*state.0
    {
        let mut processor = processor.lock().await;
        for session in closed_sessions.lock().unwrap().drain(..) {
            close_observation_processor(&mut processor, session);
        }
        process_observation_processor(&mut processor, state, session, &mut response).await?;
    }
    super::disasm::decode_response(&mut response)?;
    super::privacy::scrub_response(&mut response);
    Ok(response)
}

pub(super) async fn exchange_monitor_client(
    state: &MonitorClient,
    request: MonitorRequest,
) -> Result<crate::ipc::schema::MonitorResponse> {
    match &*state.0 {
        Backend::Unavailable => bail!("privileged helper unavailable"),
        Backend::Agent(_) => bail!("raw acquisition is unavailable through an agent link"),
        Backend::Remote {
            sender, shutdown, ..
        } => {
            let (reply, received) = oneshot::channel();
            sender
                .try_send(PendingRequest { request, reply })
                .map_err(|_| anyhow::anyhow!("monitor IPC unavailable or request queue full"))?;
            let response = match tokio::time::timeout(TIMEOUT, received).await {
                Ok(response) => response.context("monitor IPC disconnected")?,
                Err(error) => {
                    let _ = shutdown.shutdown(Shutdown::Both);
                    return Err(error).context("monitor IPC request timed out");
                }
            };
            response.map_err(anyhow::Error::msg)
        }
    }
}

pub(super) async fn observe_monitor_client(
    state: &MonitorClient,
    request: MonitorRequest,
) -> Result<ObservationSession> {
    match request_monitor_client(state, request).await? {
        MonitorResponse::Started { session, initial } => Ok(ObservationSession {
            client: state.clone(),
            session,
            initial: Some(initial),
        }),
        _ => bail!("unexpected monitor subscription response"),
    }
}

/// Queues a session close without waiting for the reply.
pub(super) fn close_monitor_client(state: &MonitorClient, session: u64) {
    use crate::http_server::{
        hosts::close_session_agent_link, processing::close_observation_processor,
    };

    if let Backend::Remote {
        processor,
        closed_sessions,
        ..
    } = &*state.0
    {
        closed_sessions.lock().unwrap().push(session);
        if let Ok(mut processor) = processor.try_lock() {
            for session in closed_sessions.lock().unwrap().drain(..) {
                close_observation_processor(&mut processor, session);
            }
        }
    }
    match &*state.0 {
        Backend::Unavailable => {}
        Backend::Agent(link) => close_session_agent_link(link, session),
        Backend::Remote { sender, .. } => {
            let (reply, _received) = oneshot::channel();
            // Keep the receiver alive until after enqueue; unlike normal
            // requests, Close is always dispatched even without a receiver.
            let _ = sender.try_send(PendingRequest {
                request: MonitorRequest::Close { session },
                reply,
            });
        }
    }
}

/// Owns a parent's observation session for the complete HTTP stream lifetime.
/// Drop queues cancellation without blocking a Tokio worker. Lease expiry is
/// the fallback if the bounded queue is full or setup was cancelled in flight.
pub(super) struct ObservationSession {
    client: MonitorClient,
    session: u64,
    pub(super) initial: Option<MonitorEvent>,
}
pub(super) async fn poll_observation_session(
    state: &ObservationSession,
) -> Result<(Option<MonitorEvent>, bool)> {
    match request_monitor_client(
        &state.client,
        MonitorRequest::Poll {
            session: state.session,
        },
    )
    .await?
    {
        MonitorResponse::Event { event, closed } => Ok((event, closed)),
        _ => bail!("unexpected monitor event response"),
    }
}
impl Drop for ObservationSession {
    fn drop(&mut self) {
        close_monitor_client(&self.client, self.session);
    }
}
