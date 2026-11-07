//! The agent side of a hub link: reconnects with backoff and answers the hub's
//! requests with this host's helper.

use super::identity::HostInfo;
use super::listener::{IDLE_TIMEOUT, PING_INTERVAL};
use super::wire::AgentResponse;
use super::wire::{self, Message};
use crate::http_server::client::MonitorClient;
use crate::{http_server::protocol::MonitorResponse, ipc::schema::MonitorRequest};
use anyhow::{Context, Result, bail};
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    net::TcpStream,
    sync::{Semaphore, mpsc, watch},
    time::timeout,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const MIN_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);
/// A session that lasted this long resets the reconnect backoff.
const STABLE_SESSION: Duration = Duration::from_secs(30);
/// Hub requests answered at once, matching the helper's request queue.
const CONCURRENT_REQUESTS: usize = 64;

/// Keeps a link to the hub at `address` until `shutdown` changes.
pub(in crate::http_server) async fn run_hub_connector(
    address: String,
    info: HostInfo,
    token: Option<Arc<str>>,
    client: MonitorClient,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut backoff = MIN_BACKOFF;
    loop {
        let started = Instant::now();
        let result = tokio::select! {
            result = run_agent_session(&address, &info, token.as_deref(), &client) => result,
            _ = shutdown.changed() => return,
        };
        match result {
            Ok(()) => log::info!("hub {address} closed the agent link"),
            Err(error) => log::warn!("agent link to hub {address}: {error:#}"),
        }
        if started.elapsed() >= STABLE_SESSION {
            backoff = MIN_BACKOFF;
        }
        // Up to 20% jitter keeps many agents from reconnecting in step.
        let jitter = backoff.mul_f64(f64::from(std::process::id() % 21) / 100.0);
        tokio::select! {
            _ = tokio::time::sleep(backoff + jitter) => {}
            _ = shutdown.changed() => return,
        }
        backoff = (backoff * 2).min(MAX_BACKOFF);
    }
}

/// Observation sessions this link started. The hub may poll and close only these,
/// never the sessions of this host's own viewers.
#[derive(Default)]
struct Sessions(Mutex<HashSet<u64>>);
impl Sessions {
    fn contains(&self, session: u64) -> bool {
        self.0.lock().unwrap().contains(&session)
    }
    /// Records a started session, and forgets `used` once it is closed or failed.
    fn update(&self, used: Option<u64>, closing: bool, response: &AgentResponse) {
        let mut sessions = self.0.lock().unwrap();
        if let Ok(MonitorResponse::Started { session, .. }) = response {
            sessions.insert(*session);
        }
        let ended = closing
            || matches!(
                response,
                Ok(MonitorResponse::Event { closed: true, .. }) | Err(_)
            );
        if let Some(session) = used
            && ended
        {
            sessions.remove(&session);
        }
    }
    fn drain(&self) -> Vec<u64> {
        self.0.lock().unwrap().drain().collect()
    }
}

async fn answer_monitor_request(
    client: &MonitorClient,
    sessions: &Sessions,
    request: MonitorRequest,
) -> AgentResponse {
    let (used, closing) = match &request {
        MonitorRequest::Poll { session } => (Some(*session), false),
        MonitorRequest::Close { session } => (Some(*session), true),
        _ => (None, false),
    };
    if let Some(session) = used
        && !sessions.contains(session)
    {
        return Err("unknown observation session".into());
    }
    let response = client
        .request(request)
        .await
        .map_err(|error| format!("{error:#}"));
    sessions.update(used, closing, &response);
    response
}

async fn run_agent_session(
    address: &str,
    info: &HostInfo,
    token: Option<&str>,
    client: &MonitorClient,
) -> Result<()> {
    let mut stream = timeout(CONNECT_TIMEOUT, TcpStream::connect(address))
        .await
        .context("connect timed out")??;
    stream.set_nodelay(true)?;
    wire::write_host_message(
        &mut stream,
        Message::Hello {
            protocol: wire::PROTOCOL,
            host: info.name.clone(),
            machine_id: info.machine_id.clone(),
            version: info.version.clone(),
            kernel: info.kernel.clone(),
            arch: info.arch.clone(),
            token: token.map(str::to_owned),
        },
    )
    .await?;
    match timeout(CONNECT_TIMEOUT, wire::read_host_message(&mut stream))
        .await
        .context("hub welcome timed out")??
    {
        Message::Welcome { .. } => {}
        Message::Reject { reason } => bail!("hub rejected this agent: {reason}"),
        _ => bail!("hub did not answer hello"),
    }
    log::info!("agent {} linked to hub {address}", info.name);
    let (mut reader, mut writer) = stream.into_split();
    let (outgoing, mut sending) = mpsc::channel::<Message>(CONCURRENT_REQUESTS + 8);
    let writing = tokio::spawn(async move {
        while let Some(message) = sending.recv().await {
            wire::write_host_message(&mut writer, message).await?;
        }
        anyhow::Ok(())
    });
    let (messages, mut incoming) = mpsc::channel(8);
    let reading = tokio::spawn(async move {
        loop {
            let message = wire::read_host_message(&mut reader).await;
            let failed = message.is_err();
            if messages.send(message).await.is_err() || failed {
                break;
            }
        }
    });
    let sessions = Arc::new(Sessions::default());
    let permits = Arc::new(Semaphore::new(CONCURRENT_REQUESTS));
    let mut ping = tokio::time::interval(PING_INTERVAL);
    let mut last_message = Instant::now();
    let result: Result<()> = async {
        loop {
            tokio::select! {
                message = incoming.recv() => {
                    let Some(message) = message else { return Ok(()) };
                    last_message = Instant::now();
                    match message? {
                        Message::Request { id, request } => {
                            let permit = permits
                                .clone()
                                .try_acquire_owned()
                                .map_err(|_| anyhow::anyhow!("hub exceeded the request limit"))?;
                            let (client, sessions, outgoing) =
                                (client.clone(), sessions.clone(), outgoing.clone());
                            tokio::spawn(async move {
                                let response = answer_monitor_request(&client, &sessions, request).await;
                                let _ = outgoing.send(Message::Response { id, response }).await;
                                drop(permit);
                            });
                        }
                        Message::Ping => drop(outgoing.try_send(Message::Pong)),
                        Message::Pong => {}
                        _ => bail!("unexpected message from hub"),
                    }
                }
                _ = ping.tick() => {
                    if writing.is_finished() {
                        bail!("hub link writer stopped");
                    }
                    if last_message.elapsed() > IDLE_TIMEOUT {
                        bail!("hub link idle for {IDLE_TIMEOUT:?}");
                    }
                    drop(outgoing.try_send(Message::Ping));
                }
            }
        }
    }
    .await;
    reading.abort();
    writing.abort();
    // Observation sessions of this link have no reader anymore.
    for session in sessions.drain() {
        client.close(session);
    }
    result
}
