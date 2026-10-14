use super::{protocol::*, transport};
use crate::http_server::{process, system};
use anyhow::{Result, ensure};
use serde_json::to_value;
use std::{
    collections::HashMap,
    os::{fd::AsRawFd, unix::net::UnixStream},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Owns all acquisition workers and privileged descriptors in the parent.
pub(in crate::http_server) struct MonitorState {
    pub(in crate::http_server) system_monitor: Arc<system::SystemMonitor>,
    pub(in crate::http_server) discovery: Mutex<process::ProcessScanner>,
    pub(in crate::http_server) monitoring: Arc<process::ProcessMonitor>,
}
impl MonitorState {
    pub(in crate::http_server) fn new(interval: Duration) -> Self {
        Self {
            system_monitor: Arc::new(system::SystemMonitor::default()),
            discovery: Mutex::new(process::ProcessScanner::default()),
            monitoring: Arc::new(process::ProcessMonitor::new(interval)),
        }
    }
    pub(in crate::http_server) fn stop(&self) {
        self.monitoring.stop();
        self.system_monitor.stop();
    }
    pub(in crate::http_server) fn join_collectors(&self) -> Result<()> {
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

/// Serial dispatch bounds in-flight work to one operation. Long-lived sampling
/// is owned by independently bounded monitor sessions, never by HTTP tasks.
pub(super) struct MonitorService {
    pub(super) state: Arc<MonitorState>,
    sessions: HashMap<u64, ObservationLease>,
    next_session: u64,
}
impl MonitorService {
    pub(super) fn new(state: Arc<MonitorState>) -> Self {
        Self {
            state,
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
        ensure!(!self.state.monitoring.is_stopped(), "Server is stopping");
        match request {
            MonitorRequest::Processes => Ok(MonitorResponse::Data(to_value(
                self.state.discovery.lock().unwrap().collect()?,
            )?)),
            MonitorRequest::Process { id, resource } => {
                // Never rely on the HTTP child's query validation. Recheck
                // identity after acquisition, too, before releasing the result.
                process::check_identity(id)?;
                let value = match resource {
                    ProcessResource::Observation => to_value(process::observation(id)?)?,
                    ProcessResource::Threads => to_value(process::threads(id)?)?,
                    ProcessResource::Maps => to_value(process::maps(id)?)?,
                    ProcessResource::Environment => to_value(process::environment(id)?)?,
                    ProcessResource::Auxv => to_value(process::auxv(id)?)?,
                    ProcessResource::Fds => to_value(process::fds(id)?)?,
                };
                process::check_identity(id)?;
                Ok(MonitorResponse::Data(value))
            }
            MonitorRequest::ObserveProcess { id } => {
                process::check_identity(id)?;
                let permit = self.state.monitoring.reserve().map_err(|e| {
                    anyhow::anyhow!(
                        "{}",
                        match e {
                            process::SubscribeError::Stopped => "Server is stopping",
                            process::SubscribeError::TooManySubscribers => "Too many viewers",
                        }
                    )
                })?;
                let mut subscription = self.state.monitoring.observe(id, permit)?;
                let initial = to_value(subscription.receiver.borrow_and_update().clone())?;
                self.start(
                    Subscription::Process(subscription),
                    MonitorEvent::Observation(initial),
                )
            }
            MonitorRequest::ObserveSystem => {
                let subscription = self.state.system_monitor.subscribe().map_err(|e| {
                    anyhow::anyhow!(
                        "{}",
                        match e {
                            system::SubscribeError::Stopped => "Server is stopping",
                            system::SubscribeError::TooManySubscribers => "Too many viewers",
                        }
                    )
                })?;
                let initial = MonitorEvent::Snapshot((*subscription.initial).clone());
                self.start(Subscription::System(subscription), initial)
            }
            MonitorRequest::Poll { session } => {
                let state = self
                    .sessions
                    .get_mut(&session)
                    .ok_or_else(|| anyhow::anyhow!("unknown or expired observation session"))?;
                state.polled = Instant::now();
                let mut closed = false;
                let event = match &mut state.subscription {
                    Subscription::Process(subscription) => {
                        match subscription.receiver.has_changed() {
                            Ok(true) => {
                                let data = subscription.receiver.borrow_and_update().clone();
                                closed = data.exited;
                                Some(MonitorEvent::Observation(to_value(data)?))
                            }
                            Ok(false) => None,
                            Err(_) => {
                                closed = true;
                                None
                            }
                        }
                    }
                    Subscription::System(subscription) => match subscription.receiver.try_recv() {
                        Ok(system::SystemMonitorEvent::Snapshot(data)) => {
                            Some(MonitorEvent::Snapshot((*data).clone()))
                        }
                        Ok(system::SystemMonitorEvent::Activity(data)) => {
                            Some(MonitorEvent::Activity(to_value(&*data)?))
                        }
                        Err(tokio::sync::broadcast::error::TryRecvError::Lagged(dropped)) => {
                            Some(MonitorEvent::Gap {
                                dropped,
                                snapshot: (*self.state.system_monitor.snapshot()).clone(),
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
            if let Err(error) = transport::write(&mut socket, &response, MAX_RESPONSE) {
                if error.downcast_ref::<serde_json::Error>().is_some() {
                    let response: WireResponse = Err("monitor response exceeds size limit".into());
                    transport::write(&mut socket, &response, MAX_RESPONSE)?;
                } else {
                    return Err(error);
                }
            }
        }
    }
}
