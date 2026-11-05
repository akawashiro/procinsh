use super::helper_service::{CollectorManager, HelperIpcService};
use crate::ipc::{schema::*, transport};
use crate::privileged_helper::process::{self, ProcessId};
use std::{io::Write, os::unix::net::UnixStream, sync::Arc, time::Duration};

#[test]
fn validates_identity_before_creating_collectors_and_releases_sessions() {
    let collectors = Arc::new(CollectorManager::new(Duration::from_millis(100)));
    let mut ipc_service = HelperIpcService::new(collectors.clone());
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
            ipc_service
                .dispatch(MonitorRequest::Process {
                    id,
                    resource: ProcessResource::Maps
                })
                .is_err()
        );
        assert!(
            ipc_service
                .dispatch(MonitorRequest::ObserveProcess { id })
                .is_err()
        );
        assert_eq!(collectors.monitoring.observer_count(), 0);
    }
    let target = process::Target::new("sleeping");
    let MonitorResponse::Started { session, .. } = ipc_service
        .dispatch(MonitorRequest::ObserveProcess { id: target.id })
        .unwrap()
    else {
        panic!("expected session")
    };
    assert_eq!(collectors.monitoring.observer_count(), 1);
    ipc_service
        .dispatch(MonitorRequest::Close { session })
        .unwrap();
    assert_eq!(collectors.monitoring.observer_count(), 0);
    assert!(
        ipc_service
            .dispatch(MonitorRequest::Poll { session })
            .is_err()
    );
    let _ = ipc_service
        .dispatch(MonitorRequest::ObserveProcess { id: target.id })
        .unwrap();
    drop(ipc_service);
    assert_eq!(collectors.monitoring.observer_count(), 0);
    collectors.stop();
    collectors.join_collectors().unwrap();
}

#[test]
fn disconnect_drops_all_parent_sessions() {
    let collectors = Arc::new(CollectorManager::new(Duration::from_millis(100)));
    let (mut client, server) = UnixStream::pair().unwrap();
    let ipc_service = HelperIpcService::new(collectors.clone());
    let worker = std::thread::spawn(move || ipc_service.serve(server));
    let target = process::Target::new("sleeping");
    transport::send(
        &mut client,
        &MonitorRequest::ObserveProcess { id: target.id },
        MAX_REQUEST,
    )
    .unwrap();
    let response: WireResponse = transport::read(&mut client, MAX_RESPONSE).unwrap();
    assert!(matches!(response, Ok(MonitorResponse::Started { .. })));
    assert_eq!(collectors.monitoring.observer_count(), 1);
    drop(client);
    assert!(worker.join().unwrap().is_err());
    assert_eq!(collectors.monitoring.observer_count(), 0);
    collectors.stop();
    collectors.join_collectors().unwrap();
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
