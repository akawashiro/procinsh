//! Every host on one SPACE stream. Snapshot, activity and gap payloads carry

//! `"host"`; `hosts` lists the hosts and `links` the connections between them.
//! Watching the merged view starts collection on every online agent.

use super::super::{
    AppState,
    client::{MonitorClient, ObservationSession},
    delivery::SnapshotEncoder,
    hosts::{AgentLink, correlate},
};
use super::process::ApiError;
use crate::http_server::protocol::MonitorRequest;
use axum::response::{
    IntoResponse, Response, Sse,
    sse::{Event, KeepAlive},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashMap},
    convert::Infallible,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{sync::mpsc, task::JoinSet};

/// Adds `"host"` to a JSON object payload.
fn with_host(host: &str, data: &str) -> String {
    let host = serde_json::to_string(host).unwrap();
    match data.strip_prefix('{') {
        Some(rest) if rest.trim_start().starts_with('}') => format!("{{\"host\":{host}}}"),
        Some(rest) => format!("{{\"host\":{host},{rest}"),
        None => data.to_owned(),
    }
}

type Outgoing = mpsc::Sender<(&'static str, String)>;
/// The latest complete snapshot of each forwarded host, for correlation.
type Latest = Arc<Mutex<BTreeMap<String, Arc<Value>>>>;

/// Forwards one host's observation session until it closes or fails.
async fn forward(
    state: Arc<AppState>,
    mut session: ObservationSession,
    host: String,
    out: Outgoing,
    latest: Latest,
) {
    let mut delivery = SnapshotEncoder::default();
    let mut event = session.initial.take();
    let mut closed = false;
    loop {
        let idle = event.is_none();
        if let Some(event) = event.take() {
            let (messages, snapshot) = super::stream::encode(event, &mut delivery);
            if let Some(snapshot) = snapshot {
                latest.lock().unwrap().insert(host.clone(), snapshot);
            }
            for (name, data) in messages {
                if out.send((name, with_host(&host, &data))).await.is_err() {
                    return;
                }
            }
        }
        if closed || state.stopped() {
            break;
        }
        if idle {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        match session.poll().await {
            Ok(next) => (event, closed) = next,
            Err(error) => {
                log::debug!("merged view lost host {host}: {error:#}");
                break;
            }
        }
    }
    latest.lock().unwrap().remove(&host);
}

/// Starts forwarding a remote host; failures leave the host out until it reconnects.
async fn forward_remote(
    state: Arc<AppState>,
    link: Arc<AgentLink>,
    host: String,
    out: Outgoing,
    latest: Latest,
) {
    match MonitorClient::agent(link)
        .observe(MonitorRequest::ObserveSystem)
        .await
    {
        Ok(session) => forward(state, session, host, out, latest).await,
        Err(error) => log::warn!("merged view skips host {host}: {error:#}"),
    }
}

enum Step {
    Message(Option<(&'static str, String)>),
    Hosts,
    Tick,
}

pub(super) async fn response(s: Arc<AppState>) -> Result<Response, ApiError> {
    // The local host fails the request like the single-host view does.
    let local = s.client.observe(MonitorRequest::ObserveSystem).await?;
    let local_name = s.hosts.local().name.clone();
    let (out, mut incoming) = mpsc::channel(64);
    let latest = Latest::default();
    // Dropping the stream drops the set, which aborts every forwarder and its session.
    let mut forwarders = JoinSet::new();
    forwarders.spawn(forward(
        s.clone(),
        local,
        local_name.clone(),
        out.clone(),
        latest.clone(),
    ));
    let mut changes = s.hosts.changes();
    let stream = async_stream::stream! {
        // The link each remote host is forwarded from; a reconnect gets a new forwarder.
        let mut forwarded: HashMap<String, Arc<AgentLink>> = HashMap::new();
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        let mut links = String::new();
        let mut step = Step::Hosts;
        loop {
            if s.stopped() { break; }
            match step {
                Step::Message(Some((event, data))) => {
                    yield Ok::<_, Infallible>(Event::default().event(event).data(data));
                }
                Step::Message(None) => break,
                Step::Hosts => {
                    changes.mark_unchanged();
                    for (name, link) in s.hosts.online() {
                        if forwarded.get(&name).is_some_and(|known| Arc::ptr_eq(known, &link)) {
                            continue;
                        }
                        forwarded.insert(name.clone(), link.clone());
                        forwarders.spawn(forward_remote(s.clone(), link, name, out.clone(), latest.clone()));
                    }
                    let hosts = json!({"local": local_name, "hosts": s.hosts.summaries()});
                    yield Ok(Event::default().event("hosts").data(hosts.to_string()));
                }
                Step::Tick => {
                    let snapshots: Vec<(String, Arc<Value>)> = latest
                        .lock()
                        .unwrap()
                        .iter()
                        .map(|(name, snapshot)| (name.clone(), snapshot.clone()))
                        .collect();
                    let hosts: Vec<_> = snapshots.iter().map(|(name, snapshot)| (name.as_str(), &**snapshot)).collect();
                    let current = serde_json::to_string(&correlate(&hosts)).unwrap();
                    if current != links {
                        links = current;
                        yield Ok(Event::default().event("links").data(links.clone()));
                    }
                }
            }
            step = tokio::select! {
                message = incoming.recv() => Step::Message(message),
                Ok(()) = changes.changed() => Step::Hosts,
                _ = tick.tick() => Step::Tick,
            };
        }
        drop(forwarders);
    };
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
        .into_response())
}

#[cfg(test)]
mod tests {
    #[test]
    fn host_is_added_to_object_payloads() {
        assert_eq!(super::with_host("a", r#"{"x":1}"#), r#"{"host":"a","x":1}"#);
        assert_eq!(super::with_host("a", "{}"), r#"{"host":"a"}"#);
        assert_eq!(super::with_host("a\"b", "{}"), r#"{"host":"a\"b"}"#);
    }
}
