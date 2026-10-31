use super::hosts::AgentLink;
use super::protocol::*;
use crate::ipc::transport;
use anyhow::{Context, Result, bail};
use std::{net::Shutdown, os::unix::net::UnixStream, sync::Arc};
use tokio::sync::{mpsc, oneshot};

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
impl MonitorClient {
    pub(super) fn agent(link: Arc<AgentLink>) -> Self {
        Self(Arc::new(Backend::Agent(link)))
    }
    pub(super) fn unavailable() -> Self {
        Self(Arc::new(Backend::Unavailable))
    }
    pub(super) fn new(mut socket: UnixStream) -> Result<Self> {
        transport::configure(&socket)?;
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
                    let response = transport::write(&mut socket, &pending.request, MAX_REQUEST)
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
        Ok(Self(Arc::new(Backend::Remote {
            sender,
            shutdown,
            processor: Box::new(tokio::sync::Mutex::new(Default::default())),
            closed_sessions: std::sync::Mutex::new(Vec::new()),
        })))
    }

    pub(super) async fn request(&self, request: MonitorRequest) -> Result<MonitorResponse> {
        if let Backend::Agent(link) = &*self.0 {
            let mut response = link.request(request).await?;
            super::privacy::scrub_response(&mut response);
            return Ok(response);
        }
        let session = match &request {
            MonitorRequest::Poll { session } => Some(*session),
            _ => None,
        };
        let mut response = super::protocol::response(self.exchange(request).await?)?;
        if let Backend::Remote {
            processor,
            closed_sessions,
            ..
        } = &*self.0
        {
            let mut processor = processor.lock().await;
            for session in closed_sessions.lock().unwrap().drain(..) {
                processor.close(session);
            }
            processor.process(self, session, &mut response).await?;
        }
        super::disasm::decode_response(&mut response)?;
        super::privacy::scrub_response(&mut response);
        Ok(response)
    }

    pub(super) async fn exchange(
        &self,
        request: MonitorRequest,
    ) -> Result<crate::ipc::schema::MonitorResponse> {
        match &*self.0 {
            Backend::Unavailable => bail!("privileged helper unavailable"),
            Backend::Agent(_) => bail!("raw acquisition is unavailable through an agent link"),
            Backend::Remote {
                sender, shutdown, ..
            } => {
                let (reply, received) = oneshot::channel();
                sender
                    .try_send(PendingRequest { request, reply })
                    .map_err(|_| {
                        anyhow::anyhow!("monitor IPC unavailable or request queue full")
                    })?;
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

    pub(super) async fn observe(&self, request: MonitorRequest) -> Result<ObservationSession> {
        match self.request(request).await? {
            MonitorResponse::Started { session, initial } => Ok(ObservationSession {
                client: self.clone(),
                session,
                initial: Some(initial),
            }),
            _ => bail!("unexpected monitor subscription response"),
        }
    }

    /// Queues a session close without waiting for the reply.
    pub(super) fn close(&self, session: u64) {
        if let Backend::Remote {
            processor,
            closed_sessions,
            ..
        } = &*self.0
        {
            closed_sessions.lock().unwrap().push(session);
            if let Ok(mut processor) = processor.try_lock() {
                for session in closed_sessions.lock().unwrap().drain(..) {
                    processor.close(session);
                }
            }
        }
        match &*self.0 {
            Backend::Unavailable => {}
            Backend::Agent(link) => link.close_session(session),
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
}

/// Owns a parent's observation session for the complete HTTP stream lifetime.
/// Drop queues cancellation without blocking a Tokio worker. Lease expiry is
/// the fallback if the bounded queue is full or setup was cancelled in flight.
pub(super) struct ObservationSession {
    client: MonitorClient,
    session: u64,
    pub(super) initial: Option<MonitorEvent>,
}
impl ObservationSession {
    pub(super) async fn poll(&self) -> Result<(Option<MonitorEvent>, bool)> {
        match self
            .client
            .request(MonitorRequest::Poll {
                session: self.session,
            })
            .await?
        {
            MonitorResponse::Event { event, closed } => Ok((event, closed)),
            _ => bail!("unexpected monitor event response"),
        }
    }
}
impl Drop for ObservationSession {
    fn drop(&mut self) {
        self.client.close(self.session);
    }
}
