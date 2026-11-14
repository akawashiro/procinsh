//! Serve helper IPC requests and manage collection workers and observation leases.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`CollectorManager`] | `pub(in crate::privileged_helper)` | `struct CollectorManager` |
//! | [`HelperIpcService`] | `pub(super)` | `struct HelperIpcService` |
//! | [`create_collector_manager`] | `pub(super)` | `fn create_collector_manager( interval: `[`Duration`]`, ) -> `[`CollectorManager`] |
//! | [`stop_collector_manager`] | `pub(super)` | `fn stop_collector_manager(state: &`[`CollectorManager`]`)` |
//! | [`join_collection_workers`] | `pub(super)` | `fn join_collection_workers( state: &`[`CollectorManager`]`, ) -> `[`Result`]`<()>` |
//! | [`create_helper_ipc_service`] | `pub(super)` | `fn create_helper_ipc_service(collectors: `[`Arc`]`<`[`CollectorManager`]`>) -> `[`HelperIpcService`] |
//! | [`dispatch_helper_ipc_service`] | `pub(super)` | `fn dispatch_helper_ipc_service( state: &mut `[`HelperIpcService`]`, request: `[`MonitorRequest`]`, ) -> `[`Result`]`<`[`MonitorResponse`]`>` |
//! | [`serve_helper_ipc_service`] | `pub(super)` | `fn serve_helper_ipc_service( mut state: `[`HelperIpcService`]`, mut socket: `[`UnixStream`]`, ) -> `[`Result`]`<()>` |

use crate::ipc::{schema::*, transport};
use crate::privileged_helper::{process, system};
use anyhow::{Result, ensure};
use std::{
    collections::HashMap,
    os::{fd::AsRawFd, unix::net::UnixStream},
    sync::Arc,
    time::{Duration, Instant},
};

/// Owns all collection workers and privileged descriptors in the helper.
pub(in crate::privileged_helper) struct CollectorManager {
    system_monitor: Arc<system::SystemMonitor>,
    pub(super) monitoring: Arc<process::ProcessMonitor>,
}
pub(super) fn create_collector_manager(interval: Duration) -> CollectorManager {
    use crate::privileged_helper::process::create_process_monitor;

    CollectorManager {
        system_monitor: Arc::new(system::SystemMonitor::default()),
        monitoring: Arc::new(create_process_monitor(interval)),
    }
}

pub(super) fn stop_collector_manager(state: &CollectorManager) {
    use crate::privileged_helper::{process::stop_process_monitor, system::stop_system_monitor};

    stop_process_monitor(&state.monitoring);
    stop_system_monitor(&state.system_monitor);
}

pub(super) fn join_collection_workers(state: &CollectorManager) -> Result<()> {
    use crate::privileged_helper::{
        process::join_process_collectors, system::join_system_monitor_workers,
    };

    join_process_collectors(&state.monitoring)
        .and(join_system_monitor_workers(&state.system_monitor))
}

enum Subscription {
    Process(process::ObservationSession),
    System(system::Subscription),
}
struct ObservationLease {
    subscription: Subscription,
    polled: Instant,
}

/// Serves helper IPC requests and manages observation leases.
/// Serial dispatch bounds in-flight work to one operation. Long-lived sampling
/// is owned by independently bounded monitor sessions, never by HTTP tasks.
pub(super) struct HelperIpcService {
    collectors: Arc<CollectorManager>,
    sessions: HashMap<u64, ObservationLease>,
    next_session: u64,
}
pub(super) fn create_helper_ipc_service(collectors: Arc<CollectorManager>) -> HelperIpcService {
    HelperIpcService {
        collectors,
        sessions: HashMap::new(),
        next_session: 0,
    }
}

fn expire_helper_ipc_service(state: &mut HelperIpcService) {
    state
        .sessions
        .retain(|_, session| session.polled.elapsed() < SESSION_LEASE);
}

fn start_helper_ipc_service(
    state: &mut HelperIpcService,
    subscription: Subscription,
    initial: MonitorEvent,
) -> Result<MonitorResponse> {
    state.next_session = state
        .next_session
        .checked_add(1)
        .ok_or_else(|| anyhow::anyhow!("session IDs exhausted"))?;
    let session = state.next_session;
    state.sessions.insert(
        session,
        ObservationLease {
            subscription,
            polled: Instant::now(),
        },
    );
    Ok(MonitorResponse::Started { session, initial })
}

pub(super) fn dispatch_helper_ipc_service(
    state: &mut HelperIpcService,
    request: MonitorRequest,
) -> Result<MonitorResponse> {
    use crate::privileged_helper::{
        process::{observe_process_monitor, process_monitor_is_stopped, reserve_process_monitor},
        system::{snapshot_system_monitor, subscribe_system_monitor},
    };

    expire_helper_ipc_service(state);
    if let MonitorRequest::Close { session } = request {
        state.sessions.remove(&session);
        return Ok(MonitorResponse::Closed);
    }
    ensure!(
        !process_monitor_is_stopped(&state.collectors.monitoring),
        "Server is stopping"
    );
    match request {
        MonitorRequest::MappedElf {
            id,
            mapping_start,
            ranges,
        } => process::acquire_process_elf_ranges(id, mapping_start, ranges),
        MonitorRequest::Processes => Ok(MonitorResponse::Data(AcquiredData::Processes(
            process::collect_processes()?,
        ))),
        MonitorRequest::Process { id, resource } => {
            process::validate_process_id(id)?;
            let data = match resource {
                ProcessResource::Observation => {
                    AcquiredData::Observation(process::collect_process_observation(id)?)
                }
                ProcessResource::Threads => {
                    AcquiredData::Observation(process::collect_thread_observation(id)?)
                }
                ProcessResource::Maps => AcquiredData::Maps(process::collect_memory_maps(id)?),
                ProcessResource::Environment => {
                    AcquiredData::Environment(process::read_environment(id)?)
                }
                ProcessResource::Auxv => AcquiredData::Auxv(process::read_auxiliary_vector(id)?),
                ProcessResource::Fds => AcquiredData::Fds(process::collect_file_descriptors(id)?),
            };
            process::validate_process_id(id)?;
            Ok(MonitorResponse::Data(data))
        }
        MonitorRequest::ObserveProcess { id } => {
            process::validate_process_id(id)?;
            let permit = reserve_process_monitor(&state.collectors.monitoring).map_err(|e| {
                anyhow::anyhow!(
                    "{}",
                    match e {
                        process::SubscribeError::Stopped => "Server is stopping",
                        process::SubscribeError::TooManySubscribers => "Too many viewers",
                    }
                )
            })?;
            let mut subscription =
                observe_process_monitor(&state.collectors.monitoring, id, permit)?;
            let initial = Arc::new(subscription.receiver.borrow_and_update().clone());
            start_helper_ipc_service(
                state,
                Subscription::Process(subscription),
                MonitorEvent::Observation(initial),
            )
        }
        MonitorRequest::ObserveSystem => {
            let subscription =
                subscribe_system_monitor(&state.collectors.system_monitor).map_err(|e| {
                    anyhow::anyhow!(
                        "{}",
                        match e {
                            system::SubscribeError::Stopped => "Server is stopping",
                            system::SubscribeError::TooManySubscribers => "Too many viewers",
                        }
                    )
                })?;
            let initial = MonitorEvent::AcquiredSnapshot(subscription.initial.clone());
            start_helper_ipc_service(state, Subscription::System(subscription), initial)
        }
        MonitorRequest::Poll { session } => {
            let lease = state
                .sessions
                .get_mut(&session)
                .ok_or_else(|| anyhow::anyhow!("unknown or expired observation session"))?;
            lease.polled = Instant::now();
            let mut closed = false;
            let event = match &mut lease.subscription {
                Subscription::Process(subscription) => {
                    match subscription.receiver.has_changed() {
                        Ok(true) => {
                            let data = subscription.receiver.borrow_and_update().clone();
                            closed = data.exited;
                            Some(MonitorEvent::Observation(Arc::new(data)))
                        }
                        Ok(false) => None,
                        Err(_) => {
                            // watch::has_changed reports closure even if the
                            // final value has not been consumed. Forward it
                            // before ending the HTTP observation stream.
                            closed = true;
                            Some(MonitorEvent::Observation(Arc::new(
                                subscription.receiver.borrow_and_update().clone(),
                            )))
                        }
                    }
                }
                Subscription::System(subscription) => match subscription.receiver.try_recv() {
                    Ok(system::SystemMonitorEvent::Snapshot(data)) => {
                        Some(MonitorEvent::AcquiredSnapshot(data))
                    }
                    Ok(system::SystemMonitorEvent::Activity(data)) => {
                        Some(MonitorEvent::Activity(data))
                    }
                    Err(tokio::sync::broadcast::error::TryRecvError::Lagged(dropped)) => {
                        Some(MonitorEvent::AcquisitionGap {
                            dropped,
                            snapshot: snapshot_system_monitor(&state.collectors.system_monitor),
                        })
                    }
                    Err(tokio::sync::broadcast::error::TryRecvError::Empty) => None,
                    Err(tokio::sync::broadcast::error::TryRecvError::Closed) => {
                        closed = true;
                        None
                    }
                },
            };
            if closed {
                state.sessions.remove(&session);
            }
            Ok(MonitorResponse::Event { event, closed })
        }
        MonitorRequest::Close { .. } => unreachable!(),
    }
}

/// Poll for idle input so expired sessions are also reaped when the child
/// stops sending requests. Once a frame begins, socket timeouts apply.
pub(super) fn serve_helper_ipc_service(
    mut state: HelperIpcService,
    mut socket: UnixStream,
) -> Result<()> {
    transport::configure_ipc_socket(&socket)?;
    loop {
        expire_helper_ipc_service(&mut state);
        let mut fd = libc::pollfd {
            fd: socket.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let ready = unsafe { libc::poll(&mut fd, 1, 250) };
        if ready == 0 {
            continue;
        }
        if ready < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error.into());
        }
        let request = transport::read(&mut socket, MAX_REQUEST)?;
        let response: WireResponse =
            dispatch_helper_ipc_service(&mut state, request).map_err(|e| format!("{e:#}"));
        // Oversize replies return a bounded error rather than corrupting
        // framing or killing an otherwise usable observation connection.
        if let Err(error) = transport::send(&mut socket, &response, MAX_RESPONSE) {
            if error.is::<crate::ipc::codec::LimitExceeded>() {
                let response: WireResponse = Err("monitor response exceeds size limit".into());
                transport::send(&mut socket, &response, MAX_RESPONSE)?;
            } else {
                return Err(error);
            }
        }
    }
}

#[cfg(test)]
mod lease_tests {
    use super::*;

    #[test]
    fn unpolled_sessions_expire_and_release_collectors() {
        use crate::privileged_helper::process::{create_target, observer_count_process_monitor};

        let target = create_target("sleeping");
        let collectors = Arc::new(super::create_collector_manager(Duration::from_millis(100)));
        let mut ipc_service = super::create_helper_ipc_service(collectors.clone());
        let MonitorResponse::Started { session, .. } = super::dispatch_helper_ipc_service(
            &mut ipc_service,
            MonitorRequest::ObserveProcess { id: target.id },
        )
        .unwrap() else {
            panic!("session expected")
        };
        assert_eq!(observer_count_process_monitor(&collectors.monitoring), 1);
        ipc_service.sessions.get_mut(&session).unwrap().polled = Instant::now() - SESSION_LEASE;
        super::expire_helper_ipc_service(&mut ipc_service);
        assert_eq!(observer_count_process_monitor(&collectors.monitoring), 0);
        assert!(
            super::dispatch_helper_ipc_service(&mut ipc_service, MonitorRequest::Poll { session })
                .is_err()
        );
        super::stop_collector_manager(&collectors);
        super::join_collection_workers(&collectors).unwrap();
    }
}

#[cfg(test)]
mod exit_tests {
    use super::*;

    #[test]
    fn closed_watch_delivers_final_process_exit_before_ending_session() {
        use crate::privileged_helper::process::create_target;

        let target = create_target("sleeping");
        let collectors = Arc::new(super::create_collector_manager(Duration::from_millis(20)));
        let mut ipc_service = super::create_helper_ipc_service(collectors.clone());
        let MonitorResponse::Started { session, .. } = super::dispatch_helper_ipc_service(
            &mut ipc_service,
            MonitorRequest::ObserveProcess { id: target.id },
        )
        .unwrap() else {
            panic!("session expected")
        };
        assert_eq!(unsafe { libc::kill(target.id.pid, libc::SIGTERM) }, 0);
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let Subscription::Process(subscription) = &ipc_service.sessions[&session].subscription
            else {
                unreachable!()
            };
            if subscription.receiver.has_changed().is_err() {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "collector did not close after target exit"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        let MonitorResponse::Event {
            event: Some(MonitorEvent::Observation(data)),
            closed,
        } = super::dispatch_helper_ipc_service(&mut ipc_service, MonitorRequest::Poll { session })
            .unwrap()
        else {
            panic!("final observation expected")
        };
        assert!(closed);
        assert!(data.exited);
        super::stop_collector_manager(&collectors);
        super::join_collection_workers(&collectors).unwrap();
    }
}
