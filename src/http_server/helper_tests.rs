use super::helper_service::{MonitorService, MonitorState};
use crate::http_server::process::{self, ProcessId};
use procinsh::{protocol::*, transport};
use std::{io::Write, os::unix::net::UnixStream, sync::Arc, time::Duration};

#[test]
fn rejects_unbounded_truncated_and_unknown_requests() {
    for bytes in [
        vec![0, 0, 8, 0],
        vec![0, 0, 0, 5, b'{'],
        vec![0, 0, 0, 2, b'{', b'}'],
    ] {
        let (mut reader, mut writer) = UnixStream::pair().unwrap();
        writer.write_all(&bytes).unwrap();
        drop(writer);
        assert!(transport::read::<MonitorRequest>(&mut reader, MAX_REQUEST).is_err());
    }
    assert!(serde_json::from_str::<MonitorRequest>(r#"{"Process":{"id":{"pid":1,"start_time_ticks":1},"resource":"Maps","path":"/etc/shadow"}}"#).is_err());
}

#[test]
fn validates_identity_before_creating_collectors_and_releases_sessions() {
    let state = Arc::new(MonitorState::new(Duration::from_millis(100)));
    let mut service = MonitorService::new(state.clone());
    for id in [
        ProcessId {
            pid: -1,
            start_time_ticks: 0,
        },
        ProcessId {
            pid: std::process::id() as i32,
            start_time_ticks: u64::MAX,
        },
    ] {
        assert!(
            service
                .dispatch(MonitorRequest::Process {
                    id,
                    resource: ProcessResource::Maps
                })
                .is_err()
        );
        assert!(
            service
                .dispatch(MonitorRequest::ObserveProcess { id })
                .is_err()
        );
        assert_eq!(state.monitoring.observer_count(), 0);
    }
    let target = process::TestTarget::new("sleeping");
    let MonitorResponse::Started { session, .. } = service
        .dispatch(MonitorRequest::ObserveProcess { id: target.id })
        .unwrap()
    else {
        panic!("expected session")
    };
    assert_eq!(state.monitoring.observer_count(), 1);
    service.dispatch(MonitorRequest::Close { session }).unwrap();
    assert_eq!(state.monitoring.observer_count(), 0);
    assert!(service.dispatch(MonitorRequest::Poll { session }).is_err());
    let _ = service
        .dispatch(MonitorRequest::ObserveProcess { id: target.id })
        .unwrap();
    drop(service);
    assert_eq!(state.monitoring.observer_count(), 0);
    state.stop();
    state.join_collectors().unwrap();
}

#[test]
fn disconnect_drops_all_parent_sessions() {
    let state = Arc::new(MonitorState::new(Duration::from_millis(100)));
    let (mut client, server) = UnixStream::pair().unwrap();
    let service = MonitorService::new(state.clone());
    let worker = std::thread::spawn(move || service.serve(server));
    let target = process::TestTarget::new("sleeping");
    transport::write(
        &mut client,
        &MonitorRequest::ObserveProcess { id: target.id },
        MAX_REQUEST,
    )
    .unwrap();
    let response: WireResponse = transport::read(&mut client, MAX_RESPONSE).unwrap();
    assert!(matches!(response, Ok(MonitorResponse::Started { .. })));
    assert_eq!(state.monitoring.observer_count(), 1);
    drop(client);
    assert!(worker.join().unwrap().is_err());
    assert_eq!(state.monitoring.observer_count(), 0);
    state.stop();
    state.join_collectors().unwrap();
}

#[test]
fn a_partial_frame_cannot_extend_its_deadline() {
    let (mut reader, mut writer) = UnixStream::pair().unwrap();
    let writer = std::thread::spawn(move || {
        for byte in [0, 0, 0, 100] {
            if writer.write_all(&[byte]).is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
    });
    let started = std::time::Instant::now();
    assert!(
        transport::read_timeout::<MonitorRequest>(
            &mut reader,
            MAX_REQUEST,
            Duration::from_millis(60)
        )
        .is_err()
    );
    assert!(started.elapsed() < Duration::from_millis(200));
    drop(reader);
    writer.join().unwrap();
}
