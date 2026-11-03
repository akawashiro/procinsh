//! A hub and an agent in one process, linked over loopback TCP, each with a
//! scripted helper.

use super::{AppState, client::MonitorClient, hosts, router::router};
use crate::ipc::{
    schema::{
        AcquiredData, MAX_REQUEST, MAX_RESPONSE, MonitorEvent, MonitorRequest, MonitorResponse,
        ProcessId, WireResponse,
    },
    transport,
};
use axum::{
    body::{Body, HttpBody},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use std::{
    os::unix::net::UnixStream,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{net::TcpListener, sync::watch, time::timeout};
use tower::ServiceExt;

fn info(name: &str) -> hosts::HostInfo {
    hosts::HostInfo {
        name: name.into(),
        machine_id: format!("{name}-machine"),
        ..hosts::HostInfo::local(None)
    }
}

fn tcp(id: &str, local: (&str, u16), remote: (&str, u16)) -> Value {
    json!({"id": id, "peer": null, "shared": false, "candidate":false,"label":"TCP","endpoint":{"process_id":{"pid":7,"start_time_ticks":1},"fd":3,"fd_count":1,"resource":{"device":{"major":0,"minor":0},"inode":"1","kind":"socket"},"kind":"socket","access":"read_write","connection_status":null}, "socket": {
        "protocol": {"kind": "tcp", "family": "ipv4"}, "state": {"kind": "established"},
        "local": {"ip": local.0, "port": local.1}, "remote": {"ip": remote.0, "port": remote.1},
        "network_peer": true,"path":null,"remote_hostname":null,"remote_gid":null}})
}

/// A helper that names its host in every reply and logs the sessions it closes.
fn scripted_helper(
    host: &'static str,
    relation: Value,
    closed: Arc<Mutex<Vec<u64>>>,
) -> MonitorClient {
    let (application, mut helper) = UnixStream::pair().unwrap();
    std::thread::spawn(move || {
        let mut sessions = 0;
        while let Ok(request) = transport::read::<MonitorRequest>(&mut helper, MAX_REQUEST) {
            let response: WireResponse = match request {
                MonitorRequest::Processes => {
                    Ok(MonitorResponse::Data(AcquiredData::Processes(vec![
                        summary(
                            host,
                            ProcessId {
                                pid: 7,
                                start_time_ticks: 1,
                            },
                        ),
                    ])))
                }
                MonitorRequest::Process { id, .. } => {
                    if id.pid == 404 {
                        Err("Process exited".into())
                    } else {
                        Ok(MonitorResponse::Data(AcquiredData::Maps(
                            crate::ipc::acquisition::MemoryMaps {
                                process_id: id,
                                maps: vec![],
                                error: Some(host.into()),
                                captured_at: None,
                                rollup: None,
                            },
                        )))
                    }
                }
                MonitorRequest::ObserveSystem => {
                    sessions += 1;
                    Ok(MonitorResponse::Started {
                        session: sessions,
                        initial: MonitorEvent::AcquiredSnapshot(Arc::new(
                            crate::ipc::model::system::SystemSnapshot {
                                fd_relations: vec![
                                    serde_json::from_value(relation.clone()).unwrap(),
                                ],
                                warnings: vec![host.into()],
                                ..Default::default()
                            },
                        )),
                    })
                }
                MonitorRequest::ObserveProcess { id } => {
                    sessions += 1;
                    Ok(MonitorResponse::Started {
                        session: sessions,
                        initial: MonitorEvent::Observation(Arc::new(detail(host, id))),
                    })
                }
                MonitorRequest::MappedElf { .. } => Ok(MonitorResponse::Elf(None)),
                MonitorRequest::Poll { .. } => Ok(MonitorResponse::Event {
                    event: None,
                    closed: false,
                }),
                MonitorRequest::Close { session } => {
                    closed.lock().unwrap().push(session);
                    Ok(MonitorResponse::Closed)
                }
            };
            if transport::write(&mut helper, &response, MAX_RESPONSE).is_err() {
                break;
            }
        }
    });
    MonitorClient::new(application).unwrap()
}

fn request(path: &str) -> Request<Body> {
    Request::builder()
        .uri(path)
        .header("host", "127.0.0.1:8080")
        .body(Body::empty())
        .unwrap()
}

async fn until(what: &str, mut condition: impl FnMut() -> bool) {
    timeout(Duration::from_secs(20), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("timed out waiting for {what}"));
}

/// The next SSE event as `(name, data)`, skipping keep-alive comments.
async fn next_event(body: &mut Body) -> (String, Value) {
    loop {
        let frame = timeout(
            Duration::from_secs(20),
            std::future::poll_fn(|cx| std::pin::Pin::new(&mut *body).poll_frame(cx)),
        )
        .await
        .expect("SSE event timed out")
        .unwrap()
        .unwrap()
        .into_data()
        .unwrap();
        let text = std::str::from_utf8(&frame).unwrap();
        let name = text.lines().find_map(|l| l.strip_prefix("event: "));
        let data = text.lines().find_map(|l| l.strip_prefix("data: "));
        if let (Some(name), Some(data)) = (name, data) {
            return (name.into(), serde_json::from_str(data).unwrap());
        }
    }
}

async fn json(app: &axum::Router, path: &str) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request(path)).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

async fn events(app: &axum::Router, path: &str) -> Body {
    let response = app.clone().oneshot(request(path)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    response.into_body()
}

#[tokio::test(flavor = "multi_thread")]
async fn hub_serves_agent_processes_details_and_space_until_offline() {
    let hub_closed = Arc::new(Mutex::new(Vec::new()));
    let agent_closed = Arc::new(Mutex::new(Vec::new()));
    let hub_client = scripted_helper(
        "hub",
        tcp("x", ("10.0.0.1", 5000), ("10.0.0.2", 443)),
        hub_closed.clone(),
    );
    let agent_client = scripted_helper(
        "agent",
        tcp("y", ("10.0.0.2", 443), ("10.0.0.1", 5000)),
        agent_closed.clone(),
    );
    let registry = Arc::new(hosts::HostRegistry::new(info("hub")));
    let state = Arc::new(AppState::remote(
        Duration::from_secs(1),
        hub_client,
        registry.clone(),
    ));
    let app = router(state, "127.0.0.1:8080".parse().unwrap());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (shutdown, _) = watch::channel(false);
    let token: Option<Arc<str>> = Some("secret".into());
    tokio::spawn(hosts::run_agent_listener(
        listener,
        registry.clone(),
        token.clone(),
        shutdown.subscribe(),
    ));

    // A wrong token is rejected and never listed.
    let (stop_wrong, _) = watch::channel(false);
    let wrong = tokio::spawn(hosts::run_hub_connector(
        address.to_string(),
        info("intruder"),
        Some("guess".into()),
        MonitorClient::unavailable(),
        stop_wrong.subscribe(),
    ));
    let (stop_agent, _) = watch::channel(false);
    let agent = tokio::spawn(hosts::run_hub_connector(
        address.to_string(),
        info("agent-a"),
        token,
        agent_client,
        stop_agent.subscribe(),
    ));
    until("agent online", || {
        matches!(registry.remote("agent-a"), hosts::RemoteHost::Online(_))
    })
    .await;
    let (status, list) = json(&app, "/api/hosts").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["local"], "hub");
    let names: Vec<_> = list["hosts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|h| {
            (
                h["name"].clone(),
                h["local"].clone(),
                h["status"]["state"].clone(),
            )
        })
        .collect();
    assert_eq!(
        names,
        [
            (json!("hub"), json!(true), json!("online")),
            (json!("agent-a"), json!(false), json!("online"))
        ]
    );
    assert_eq!(list["hosts"][1]["arch"], std::env::consts::ARCH);

    // The process list and details go to the selected host's helper.
    assert_eq!(
        json(&app, "/api/processes").await.1[0]["name"],
        json!("hub")
    );
    assert_eq!(
        json(&app, "/api/processes?host=hub").await.1[0]["name"],
        json!("hub")
    );
    assert_eq!(
        json(&app, "/api/processes?host=agent-a").await.1[0]["name"],
        json!("agent")
    );
    assert_eq!(
        json(
            &app,
            "/api/processes/maps?host=agent-a&pid=7&start_time_ticks=1"
        )
        .await
        .1["error"],
        json!("agent")
    );
    let (status, error) = json(
        &app,
        "/api/processes/maps?host=agent-a&pid=404&start_time_ticks=1",
    )
    .await;
    assert_eq!(status, StatusCode::GONE, "helper errors keep their status");
    assert!(error["error"].as_str().unwrap().contains("Process exited"));
    assert_eq!(
        json(&app, "/api/processes?host=nowhere").await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        json(&app, "/api/processes?host=all").await.0,
        StatusCode::BAD_REQUEST
    );

    // Process events stream from the agent; leaving closes the agent's session.
    let mut body = events(
        &app,
        "/api/processes/events?host=agent-a&pid=7&start_time_ticks=1",
    )
    .await;
    let (name, observation) = next_event(&mut body).await;
    assert_eq!(name, "observation");
    assert_eq!(observation["summary"]["name"], "agent");
    assert_eq!(observation["summary"]["identity"]["pid"], 7);
    drop(body);
    until("agent session closed", || {
        agent_closed.lock().unwrap().contains(&1)
    })
    .await;

    // A single remote host's SPACE stream.
    let mut body = events(&app, "/api/system/events?host=agent-a").await;
    let (name, snapshot) = next_event(&mut body).await;
    assert_eq!(
        (name.as_str(), &snapshot["warnings"][0]),
        ("snapshot", &json!("agent"))
    );
    assert_eq!(snapshot["kind"], "full");
    drop(body);

    // The merged view tags each host and links the TCP connection between them.
    let mut body = events(&app, "/api/system/events?host=all").await;
    let mut origins = std::collections::BTreeMap::new();
    let mut links = None;
    let mut hosts_seen = false;
    while origins.len() < 2 || links.is_none() || !hosts_seen {
        match next_event(&mut body).await {
            (name, data) if name == "snapshot" => {
                origins.insert(
                    data["host"].as_str().unwrap().to_owned(),
                    data["warnings"][0].clone(),
                );
            }
            // Links before both snapshots arrive are empty; a later event adds the match.
            (name, data) if name == "links" && data["sockets"] != json!([]) => links = Some(data),
            (name, _) if name == "hosts" => hosts_seen = true,
            _ => {}
        }
    }
    assert_eq!(origins["hub"], "hub");
    assert_eq!(origins["agent-a"], "agent");
    assert_eq!(
        links.unwrap()["sockets"],
        json!([{"a": {"host": "agent-a", "relation": "y"}, "b": {"host": "hub", "relation": "x"}}])
    );

    // The hub cannot poll sessions it did not start on the agent.
    let hosts::RemoteHost::Online(link) = registry.remote("agent-a") else {
        panic!("agent online");
    };
    let foreign = MonitorClient::agent(link)
        .request(MonitorRequest::Poll { session: 999 })
        .await;
    assert!(format!("{:#}", foreign.err().unwrap()).contains("unknown observation session"));

    // The agent leaves: requests fail as unavailable and the host is listed offline.
    stop_agent.send_replace(true);
    agent.await.unwrap();
    until("agent offline", || {
        matches!(registry.remote("agent-a"), hosts::RemoteHost::Offline)
    })
    .await;
    assert_eq!(
        json(&app, "/api/processes?host=agent-a").await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    let (_, list) = json(&app, "/api/hosts").await;
    assert_eq!(list["hosts"][1]["status"]["state"], "offline");
    assert!(
        list["hosts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|h| h["name"] != "intruder")
    );
    loop {
        match next_event(&mut body).await {
            (name, data) if name == "hosts" && data["hosts"][1]["status"]["state"] == "offline" => {
                break;
            }
            _ => {}
        }
    }
    drop(body);
    stop_wrong.send_replace(true);
    wrong.await.unwrap();
    shutdown.send_replace(true);
}

fn summary(host: &str, identity: ProcessId) -> crate::ipc::acquisition::ProcessSummary {
    crate::ipc::acquisition::ProcessSummary {
        identity,
        parent_pid: 0,
        name: host.into(),
        executable: None,
        command_line: None,
        uid: None,
        username: None,
        euid: None,
        effective_username: None,
        state: "S".into(),
        started_at: None,
        cpu_percent: None,
        rss_bytes: 0,
        thread_count: 1,
        raw_ticks: 0,
        raw_measured_ns: 0,
    }
}
fn detail(host: &str, id: ProcessId) -> crate::ipc::acquisition::ProcessDetailSnapshot {
    let sample=serde_json::from_value(json!({"timestamp":1,"process_id":id,"rss_bytes":0,"vms_bytes":0,"minor_faults":0,"major_faults":0,"voluntary_context_switches":null,"nonvoluntary_context_switches":null,"io":null,"threads":[],"cpu":0,"nice":0,"priority":20,"ticks":0,"measured_at":0})).unwrap();
    crate::ipc::acquisition::ProcessDetailSnapshot {
        summary: summary(host, id),
        exited: false,
        error: None,
        observation: None,
        process_sample: sample,
        history: Default::default(),
        maps: vec![],
        maps_error: None,
        maps_captured_at: None,
        rollup: None,
        live_samples: vec![],
        mapping_snapshots: vec![],
        sampling_error: None,
    }
}
