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
    pub(in crate::privileged_helper) system_monitor: Arc<system::SystemMonitor>,
    pub(in crate::privileged_helper) monitoring: Arc<process::ProcessMonitor>,
}
impl CollectorManager {
    pub(in crate::privileged_helper) fn new(interval: Duration) -> Self {
        Self {
            system_monitor: Arc::new(system::SystemMonitor::default()),
            monitoring: Arc::new(process::ProcessMonitor::new(interval)),
        }
    }
    pub(in crate::privileged_helper) fn stop(&self) {
        self.monitoring.stop();
        self.system_monitor.stop();
    }
    pub(in crate::privileged_helper) fn join_collectors(&self) -> Result<()> {
        self.monitoring
            .join_collectors()
            .and(self.system_monitor.join_workers())
    }
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
    pub(super) collectors: Arc<CollectorManager>,
    sessions: HashMap<u64, ObservationLease>,
    next_session: u64,
}
impl HelperIpcService {
    pub(super) fn new(collectors: Arc<CollectorManager>) -> Self {
        Self {
            collectors,
            sessions: HashMap::new(),
            next_session: 0,
        }
    }

    fn expire(&mut self) {
        self.sessions
            .retain(|_, session| session.polled.elapsed() < SESSION_LEASE);
    }

    fn start(
        &mut self,
        subscription: Subscription,
        initial: MonitorEvent,
    ) -> Result<MonitorResponse> {
        self.next_session = self
            .next_session
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("session IDs exhausted"))?;
        let session = self.next_session;
        self.sessions.insert(
            session,
            ObservationLease {
                subscription,
                polled: Instant::now(),
            },
        );
        Ok(MonitorResponse::Started { session, initial })
    }

    pub(super) fn dispatch(&mut self, request: MonitorRequest) -> Result<MonitorResponse> {
        self.expire();
        if let MonitorRequest::Close { session } = request {
            self.sessions.remove(&session);
            return Ok(MonitorResponse::Closed);
        }
        ensure!(
            !self.collectors.monitoring.is_stopped(),
            "Server is stopping"
        );
        match request {
            MonitorRequest::MappedElf {
                id,
                mapping_start,
                ranges,
            } => process::mapped_elf(id, mapping_start, ranges),
            MonitorRequest::Processes => Ok(MonitorResponse::Data(AcquiredData::Processes(
                process::collect_processes()?,
            ))),
            MonitorRequest::Process { id, resource } => {
                process::validate_process_identity(id)?;
                let data = match resource {
                    ProcessResource::Observation => {
                        AcquiredData::Observation(process::observation(id)?)
                    }
                    ProcessResource::Threads => AcquiredData::Observation(process::threads(id)?),
                    ProcessResource::Maps => AcquiredData::Maps(process::maps(id)?),
                    ProcessResource::Environment => {
                        AcquiredData::Environment(process::environment(id)?)
                    }
                    ProcessResource::Auxv => AcquiredData::Auxv(process::auxv(id)?),
                    ProcessResource::Fds => AcquiredData::Fds(process::fds(id)?),
                };
                process::validate_process_identity(id)?;
                Ok(MonitorResponse::Data(data))
            }
            MonitorRequest::ObserveProcess { id } => {
                process::validate_process_identity(id)?;
                let permit = self.collectors.monitoring.reserve().map_err(|e| {
                    anyhow::anyhow!(
                        "{}",
                        match e {
                            process::SubscribeError::Stopped => "Server is stopping",
                            process::SubscribeError::TooManySubscribers => "Too many viewers",
                        }
                    )
                })?;
                let mut subscription = self.collectors.monitoring.observe(id, permit)?;
                let initial = Arc::new(subscription.receiver.borrow_and_update().clone());
                self.start(
                    Subscription::Process(subscription),
                    MonitorEvent::Observation(initial),
                )
            }
            MonitorRequest::ObserveSystem => {
                let subscription = self.collectors.system_monitor.subscribe().map_err(|e| {
                    anyhow::anyhow!(
                        "{}",
                        match e {
                            system::SubscribeError::Stopped => "Server is stopping",
                            system::SubscribeError::TooManySubscribers => "Too many viewers",
                        }
                    )
                })?;
                let initial = MonitorEvent::AcquiredSnapshot(subscription.initial.clone());
                self.start(Subscription::System(subscription), initial)
            }
            MonitorRequest::Poll { session } => {
                let lease = self
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
                                snapshot: self.collectors.system_monitor.snapshot(),
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
                    self.sessions.remove(&session);
                }
                Ok(MonitorResponse::Event { event, closed })
            }
            MonitorRequest::Close { .. } => unreachable!(),
        }
    }

    /// Poll for idle input so expired sessions are also reaped when the child
    /// stops sending requests. Once a frame begins, socket timeouts apply.
    pub(super) fn serve(mut self, mut socket: UnixStream) -> Result<()> {
        transport::configure(&socket)?;
        loop {
            self.expire();
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
            let response: WireResponse = self.dispatch(request).map_err(|e| format!("{e:#}"));
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
}

#[cfg(test)]
mod lease_tests {
    use super::*;

    #[test]
    fn unpolled_sessions_expire_and_release_collectors() {
        let target = process::Target::new("sleeping");
        let collectors = Arc::new(CollectorManager::new(Duration::from_millis(100)));
        let mut ipc_service = HelperIpcService::new(collectors.clone());
        let MonitorResponse::Started { session, .. } = ipc_service
            .dispatch(MonitorRequest::ObserveProcess { id: target.id })
            .unwrap()
        else {
            panic!("session expected")
        };
        assert_eq!(collectors.monitoring.observer_count(), 1);
        ipc_service.sessions.get_mut(&session).unwrap().polled = Instant::now() - SESSION_LEASE;
        ipc_service.expire();
        assert_eq!(collectors.monitoring.observer_count(), 0);
        assert!(
            ipc_service
                .dispatch(MonitorRequest::Poll { session })
                .is_err()
        );
        collectors.stop();
        collectors.join_collectors().unwrap();
    }
}

#[cfg(test)]
mod exit_tests {
    use super::*;

    #[test]
    fn closed_watch_delivers_final_process_exit_before_ending_session() {
        let target = process::Target::new("sleeping");
        let collectors = Arc::new(CollectorManager::new(Duration::from_millis(20)));
        let mut ipc_service = HelperIpcService::new(collectors.clone());
        let MonitorResponse::Started { session, .. } = ipc_service
            .dispatch(MonitorRequest::ObserveProcess { id: target.id })
            .unwrap()
        else {
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
        } = ipc_service
            .dispatch(MonitorRequest::Poll { session })
            .unwrap()
        else {
            panic!("final observation expected")
        };
        assert!(closed);
        assert!(data.exited);
        collectors.stop();
        collectors.join_collectors().unwrap();
    }
}
