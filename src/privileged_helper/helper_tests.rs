use crate::ipc::schema::ProcessId;
use crate::ipc::{schema::*, transport};
use std::{io::Write, os::unix::net::UnixStream, sync::Arc, time::Duration};

#[test]
fn validates_identity_before_creating_collectors_and_releases_sessions() {
    use crate::privileged_helper::{
        helper_service::{
            create_collector_manager, create_helper_ipc_service, dispatch_helper_ipc_service,
            join_collection_workers, stop_collector_manager,
        },
        process::{create_target, observer_count_process_monitor},
    };

    let collectors = Arc::new(create_collector_manager(Duration::from_millis(100)));
    let mut ipc_service = create_helper_ipc_service(collectors.clone());
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
            dispatch_helper_ipc_service(
                &mut ipc_service,
                MonitorRequest::Process {
                    id,
                    resource: ProcessResource::Maps
                }
            )
            .is_err()
        );
        assert!(
            dispatch_helper_ipc_service(&mut ipc_service, MonitorRequest::ObserveProcess { id })
                .is_err()
        );
        assert_eq!(observer_count_process_monitor(&collectors.monitoring), 0);
    }
    let target = create_target("sleeping");
    let MonitorResponse::Started { session, .. } = dispatch_helper_ipc_service(
        &mut ipc_service,
        MonitorRequest::ObserveProcess { id: target.id },
    )
    .unwrap() else {
        panic!("expected session")
    };
    assert_eq!(observer_count_process_monitor(&collectors.monitoring), 1);
    dispatch_helper_ipc_service(&mut ipc_service, MonitorRequest::Close { session }).unwrap();
    assert_eq!(observer_count_process_monitor(&collectors.monitoring), 0);
    assert!(
        dispatch_helper_ipc_service(&mut ipc_service, MonitorRequest::Poll { session }).is_err()
    );
    let _ = dispatch_helper_ipc_service(
        &mut ipc_service,
        MonitorRequest::ObserveProcess { id: target.id },
    )
    .unwrap();
    drop(ipc_service);
    assert_eq!(observer_count_process_monitor(&collectors.monitoring), 0);
    stop_collector_manager(&collectors);
    join_collection_workers(&collectors).unwrap();
}

#[test]
fn disconnect_drops_all_parent_sessions() {
    use crate::privileged_helper::{
        helper_service::{
            create_collector_manager, create_helper_ipc_service, join_collection_workers,
            serve_helper_ipc_service, stop_collector_manager,
        },
        process::{create_target, observer_count_process_monitor},
    };

    let collectors = Arc::new(create_collector_manager(Duration::from_millis(100)));
    let (mut client, server) = UnixStream::pair().unwrap();
    let ipc_service = create_helper_ipc_service(collectors.clone());
    let worker = std::thread::spawn(move || serve_helper_ipc_service(ipc_service, server));
    let target = create_target("sleeping");
    transport::send(
        &mut client,
        &MonitorRequest::ObserveProcess { id: target.id },
        MAX_REQUEST,
    )
    .unwrap();
    let response: WireResponse = transport::read(&mut client, MAX_RESPONSE).unwrap();
    assert!(matches!(response, Ok(MonitorResponse::Started { .. })));
    assert_eq!(observer_count_process_monitor(&collectors.monitoring), 1);
    drop(client);
    assert!(worker.join().unwrap().is_err());
    assert_eq!(observer_count_process_monitor(&collectors.monitoring), 0);
    stop_collector_manager(&collectors);
    join_collection_workers(&collectors).unwrap();
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
