//! The hub's handle on one connected agent: requests out, matching responses in.


use super::wire::AgentResponse;
use super::wire::Message;
use crate::http_server::protocol::{MonitorRequest, MonitorResponse, TIMEOUT};
use anyhow::{Context, Result, bail};
use std::{
    collections::HashMap,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tokio::sync::{mpsc, oneshot};

/// Requests in flight per agent, like the helper's request queue.
const PENDING_LIMIT: usize = 64;
/// The agent's own helper request has the IPC deadline; allow for the network.
const REQUEST_TIMEOUT: Duration = TIMEOUT.saturating_add(Duration::from_secs(5));

pub(in crate::http_server) struct AgentLink {
    outgoing: mpsc::Sender<Message>,
    /// `None` once the link has closed; dropping the senders fails every waiter.
    pending: Mutex<Option<HashMap<u64, oneshot::Sender<AgentResponse>>>>,
    next: AtomicU64,
}

impl AgentLink {
    /// A link whose messages for the agent are read from the returned receiver.
    pub(super) fn new() -> (Self, mpsc::Receiver<Message>) {
        let (outgoing, receiver) = mpsc::channel(PENDING_LIMIT + 8);
        let link = Self {
            outgoing,
            pending: Mutex::new(Some(HashMap::new())),
            next: AtomicU64::new(0),
        };
        (link, receiver)
    }

    pub(in crate::http_server) async fn request(
        &self,
        request: MonitorRequest,
    ) -> Result<MonitorResponse> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (reply, received) = oneshot::channel();
        {
            let mut pending = self.pending.lock().unwrap();
            let pending = pending.as_mut().context("agent link closed")?;
            if pending.len() >= PENDING_LIMIT {
                bail!("agent link request queue full");
            }
            pending.insert(id, reply);
        }
        if self
            .outgoing
            .try_send(Message::Request { id, request })
            .is_err()
        {
            self.forget(id);
            bail!("agent link closed or request queue full");
        }
        let response = match tokio::time::timeout(REQUEST_TIMEOUT, received).await {
            Ok(response) => response.context("agent link closed")?,
            Err(_) => {
                self.forget(id);
                bail!("agent link request timed out");
            }
        };
        response.map_err(anyhow::Error::msg)
    }

    /// Queues a session close without waiting; the agent's lease is the fallback.
    pub(in crate::http_server) fn close_session(&self, session: u64) {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let _ = self.outgoing.try_send(Message::Request {
            id,
            request: MonitorRequest::Close { session },
        });
    }

    /// Queues a message of the link itself, such as a ping.
    pub(super) fn send(&self, message: Message) -> Result<()> {
        self.outgoing
            .try_send(message)
            .map_err(|_| anyhow::anyhow!("agent link send queue full"))
    }

    fn forget(&self, id: u64) {
        if let Some(pending) = self.pending.lock().unwrap().as_mut() {
            pending.remove(&id);
        }
    }

    /// Delivers a response; responses to forgotten or fire-and-forget requests are dropped.
    pub(super) fn complete(&self, id: u64, response: AgentResponse) {
        let reply = self
            .pending
            .lock()
            .unwrap()
            .as_mut()
            .and_then(|pending| pending.remove(&id));
        if let Some(reply) = reply {
            let _ = reply.send(response);
        }
    }

    /// Fails every waiting request and refuses new ones.
    pub(super) fn close(&self) {
        self.pending.lock().unwrap().take();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn responses_match_requests_and_closing_fails_waiters() {
        let (link, mut outgoing) = AgentLink::new();
        let link = std::sync::Arc::new(link);
        let waiting = tokio::spawn({
            let link = link.clone();
            async move { link.request(MonitorRequest::Processes).await }
        });
        let Some(Message::Request {
            id,
            request: MonitorRequest::Processes,
        }) = outgoing.recv().await
        else {
            panic!("request expected");
        };
        link.complete(id + 100, Err("unrelated".into()));
        link.complete(id, Ok(MonitorResponse::Data(json!([1]))));
        let Ok(MonitorResponse::Data(data)) = waiting.await.unwrap() else {
            panic!("data expected");
        };
        assert_eq!(data, json!([1]));
        let failing = tokio::spawn({
            let link = link.clone();
            async move { link.request(MonitorRequest::Processes).await }
        });
        let Some(Message::Request { id, .. }) = outgoing.recv().await else {
            panic!("request expected");
        };
        link.complete(id, Err("Process exited".into()));
        assert!(format!("{:#}", failing.await.unwrap().err().unwrap()).contains("Process exited"));
        let orphan = tokio::spawn({
            let link = link.clone();
            async move { link.request(MonitorRequest::Processes).await }
        });
        outgoing.recv().await.unwrap();
        link.close();
        assert!(
            format!("{:#}", orphan.await.unwrap().err().unwrap()).contains("agent link closed")
        );
        assert!(link.request(MonitorRequest::Processes).await.is_err());
    }
}
