use anyhow::{Context, Result, bail};
use procinsh::{protocol::*, transport};
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
    },
}

/// A bounded queue and one I/O worker serialize socket access. A timed-out
/// exchange closes the connection, so replies can never match a later request.
#[derive(Clone)]
pub(super) struct MonitorClient(Arc<Backend>);
impl MonitorClient {
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
        Ok(Self(Arc::new(Backend::Remote { sender, shutdown })))
    }

    pub(super) async fn request(&self, request: MonitorRequest) -> Result<MonitorResponse> {
        match &*self.0 {
            Backend::Unavailable => bail!("privileged helper unavailable"),
            Backend::Remote { sender, shutdown } => {
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

    fn close(&self, session: u64) {
        match &*self.0 {
            Backend::Unavailable => {}
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
