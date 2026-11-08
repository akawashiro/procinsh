//! The hub's handle on one connected agent: requests out, matching responses in.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`AgentLink`] | `pub(in crate::http_server)` | `struct AgentLink` |
//! | [`create_agent_link`] | `pub(super)` | `fn create_agent_link() -> (`[`AgentLink`]`, `[`mpsc::Receiver`]`<`[`Message`]`>)` |
//! | [`request_agent_link`] | `pub(in crate::http_server)` | `async fn request_agent_link( state: &`[`AgentLink`]`, request: `[`MonitorRequest`]`, ) -> `[`Result`]`<`[`MonitorResponse`]`>` |
//! | [`close_session_agent_link`] | `pub(in crate::http_server)` | `fn close_session_agent_link(state: &`[`AgentLink`]`, session: u64)` |
//! | [`send_agent_link`] | `pub(super)` | `fn send_agent_link(state: &`[`AgentLink`]`, message: `[`Message`]`) -> `[`Result`]`<()>` |
//! | [`complete_agent_link`] | `pub(super)` | `fn complete_agent_link(state: &`[`AgentLink`]`, id: u64, response: `[`AgentResponse`]`)` |
//! | [`close_agent_link`] | `pub(super)` | `fn close_agent_link(state: &`[`AgentLink`]`)` |

use super::wire::AgentResponse;
use super::wire::Message;
use crate::{
    http_server::protocol::MonitorResponse,
    ipc::schema::{MonitorRequest, TIMEOUT},
};
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

/// A link whose messages for the agent are read from the returned receiver.
pub(super) fn create_agent_link() -> (AgentLink, mpsc::Receiver<Message>) {
    let (outgoing, receiver) = mpsc::channel(PENDING_LIMIT + 8);
    let link = AgentLink {
        outgoing,
        pending: Mutex::new(Some(HashMap::new())),
        next: AtomicU64::new(0),
    };
    (link, receiver)
}

pub(in crate::http_server) async fn request_agent_link(
    state: &AgentLink,
    request: MonitorRequest,
) -> Result<MonitorResponse> {
    let id = state.next.fetch_add(1, Ordering::Relaxed);
    let (reply, received) = oneshot::channel();
    {
        let mut pending = state.pending.lock().unwrap();
        let pending = pending.as_mut().context("agent link closed")?;
        if pending.len() >= PENDING_LIMIT {
            bail!("agent link request queue full");
        }
        pending.insert(id, reply);
    }
    if state
        .outgoing
        .try_send(Message::Request { id, request })
        .is_err()
    {
        forget_agent_link(state, id);
        bail!("agent link closed or request queue full");
    }
    let response = match tokio::time::timeout(REQUEST_TIMEOUT, received).await {
        Ok(response) => response.context("agent link closed")?,
        Err(_) => {
            forget_agent_link(state, id);
            bail!("agent link request timed out");
        }
    };
    response.map_err(anyhow::Error::msg)
}

/// Queues a session close without waiting; the agent's lease is the fallback.
pub(in crate::http_server) fn close_session_agent_link(state: &AgentLink, session: u64) {
    let id = state.next.fetch_add(1, Ordering::Relaxed);
    let _ = state.outgoing.try_send(Message::Request {
        id,
        request: MonitorRequest::Close { session },
    });
}

/// Queues a message of the link itself, such as a ping.
pub(super) fn send_agent_link(state: &AgentLink, message: Message) -> Result<()> {
    state
        .outgoing
        .try_send(message)
        .map_err(|_| anyhow::anyhow!("agent link send queue full"))
}

fn forget_agent_link(state: &AgentLink, id: u64) {
    if let Some(pending) = state.pending.lock().unwrap().as_mut() {
        pending.remove(&id);
    }
}

/// Delivers a response; responses to forgotten or fire-and-forget requests are dropped.
pub(super) fn complete_agent_link(state: &AgentLink, id: u64, response: AgentResponse) {
    let reply = state
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
pub(super) fn close_agent_link(state: &AgentLink) {
    state.pending.lock().unwrap().take();
}

#[cfg(test)]
mod tests {
    use {super::*, serde_json::json};

    #[tokio::test]
    async fn responses_match_requests_and_closing_fails_waiters() {
        let (link, mut outgoing) = super::create_agent_link();
        let link = std::sync::Arc::new(link);
        let waiting = tokio::spawn({
            let link = link.clone();
            async move { super::request_agent_link(&link, MonitorRequest::Processes).await }
        });
        let Some(Message::Request {
            id,
            request: MonitorRequest::Processes,
        }) = outgoing.recv().await
        else {
            panic!("request expected");
        };
        super::complete_agent_link(&link, id + 100, Err("unrelated".into()));
        super::complete_agent_link(&link, id, Ok(MonitorResponse::Data(json!([1]))));
        let Ok(MonitorResponse::Data(data)) = waiting.await.unwrap() else {
            panic!("data expected");
        };
        assert_eq!(data, json!([1]));
        let failing = tokio::spawn({
            let link = link.clone();
            async move { super::request_agent_link(&link, MonitorRequest::Processes).await }
        });
        let Some(Message::Request { id, .. }) = outgoing.recv().await else {
            panic!("request expected");
        };
        super::complete_agent_link(&link, id, Err("Process exited".into()));
        assert!(format!("{:#}", failing.await.unwrap().err().unwrap()).contains("Process exited"));
        let orphan = tokio::spawn({
            let link = link.clone();
            async move { super::request_agent_link(&link, MonitorRequest::Processes).await }
        });
        outgoing.recv().await.unwrap();
        super::close_agent_link(&link);
        assert!(
            format!("{:#}", orphan.await.unwrap().err().unwrap()).contains("agent link closed")
        );
        assert!(
            super::request_agent_link(&link, MonitorRequest::Processes)
                .await
                .is_err()
        );
    }
}
