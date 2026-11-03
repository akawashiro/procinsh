use super::helper_service::{MonitorService, MonitorState};
use crate::ipc::{
    schema::{
        MAX_REQUEST, MAX_RESPONSE, MonitorRequest, MonitorResponse, ProcessResource, WireResponse,
    },
    transport,
};
use crate::privileged_helper::process::{self, ProcessId};
use std::{io::Write, os::unix::net::UnixStream, sync::Arc, time::Duration};

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
