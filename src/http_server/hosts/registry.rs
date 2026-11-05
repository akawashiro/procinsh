//! Hosts known to a hub: the local machine and remote agents by name.


use super::identity::{HostInfo, valid_host_name};
use super::link::AgentLink;
use serde::Serialize;
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::watch;

const HOST_LIMIT: usize = 64;
/// Offline hosts stay listed this long.
const OFFLINE_RETENTION_MS: u64 = 10 * 60 * 1000;

fn timestamp_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub(in crate::http_server) enum HostStatus {
    /// `since` is the Unix time in milliseconds of the transition.
    Online {
        since: u64,
    },
    Offline {
        since: u64,
    },
}

/// A host list entry.
#[derive(Clone, Debug, Serialize)]
pub(in crate::http_server) struct HostSummary {
    pub(in crate::http_server) name: String,
    pub(in crate::http_server) local: bool,
    pub(in crate::http_server) status: HostStatus,
    pub(in crate::http_server) machine_id: String,
    pub(in crate::http_server) version: String,
    pub(in crate::http_server) kernel: Option<String>,
    pub(in crate::http_server) arch: String,
}

struct Remote {
    info: HostInfo,
    status: HostStatus,
    /// The current link ID and handle; a replaced link must not mark the host offline.
    link: Option<(u64, Arc<AgentLink>)>,
}

/// Whether a remote host can take requests.
pub(in crate::http_server) enum RemoteHost {
    Online(Arc<AgentLink>),
    Offline,
    Unknown,
}

pub(in crate::http_server) struct HostRegistry {
    local: HostInfo,
    started: u64,
    hosts: Mutex<BTreeMap<String, Remote>>,
    links: AtomicU64,
    /// Increments whenever a host is added, removed, or changes status.
    changes: watch::Sender<u64>,
}

impl HostRegistry {
    pub(in crate::http_server) fn new(local: HostInfo) -> Self {
        Self {
            local,
            started: timestamp_ms(),
            hosts: Mutex::new(BTreeMap::new()),
            links: AtomicU64::new(0),
            changes: watch::channel(0).0,
        }
    }

    pub(in crate::http_server) fn local(&self) -> &HostInfo {
        &self.local
    }

    pub(in crate::http_server) fn remote(&self, name: &str) -> RemoteHost {
        match self.hosts.lock().unwrap().get(name) {
            Some(Remote {
                link: Some((_, link)),
                ..
            }) => RemoteHost::Online(link.clone()),
            Some(_) => RemoteHost::Offline,
            None => RemoteHost::Unknown,
        }
    }

    /// Online remote hosts in name order.
    pub(in crate::http_server) fn online(&self) -> Vec<(String, Arc<AgentLink>)> {
        self.hosts
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(name, host)| Some((name.clone(), host.link.as_ref()?.1.clone())))
            .collect()
    }

    pub(in crate::http_server) fn changes(&self) -> watch::Receiver<u64> {
        self.changes.subscribe()
    }

    /// The local host first, then remote hosts in name order.
    pub(in crate::http_server) fn summaries(&self) -> Vec<HostSummary> {
        self.prune();
        let summary = |info: &HostInfo, local, status| HostSummary {
            name: info.name.clone(),
            local,
            status,
            machine_id: info.machine_id.clone(),
            version: info.version.clone(),
            kernel: info.kernel.clone(),
            arch: info.arch.clone(),
        };
        let mut result = vec![summary(
            &self.local,
            true,
            HostStatus::Online {
                since: self.started,
            },
        )];
        result.extend(
            self.hosts
                .lock()
                .unwrap()
                .values()
                .map(|host| summary(&host.info, false, host.status)),
        );
        result
    }

    /// Admits an agent under its name and returns the link ID. A reconnect from
    /// the same machine replaces the previous link; an online host of another
    /// machine keeps its name.
    pub(super) fn admit(&self, info: HostInfo, link: Arc<AgentLink>) -> Result<u64, String> {
        if !valid_host_name(&info.name) {
            return Err(format!("invalid host name {:?}", info.name));
        }
        if info.name == self.local.name {
            return Err(format!("host name {} is the hub's own name", info.name));
        }
        self.prune();
        let id = self.links.fetch_add(1, Ordering::Relaxed) + 1;
        let mut hosts = self.hosts.lock().unwrap();
        if let Some(host) = hosts.get(&info.name) {
            if host.link.is_some() && host.info.machine_id != info.machine_id {
                return Err(format!(
                    "host name {} is already connected from another machine",
                    info.name
                ));
            }
        } else if hosts.len() >= HOST_LIMIT {
            return Err(format!("the hub accepts at most {HOST_LIMIT} hosts"));
        }
        let previous = hosts.insert(
            info.name.clone(),
            Remote {
                info,
                status: HostStatus::Online {
                    since: timestamp_ms(),
                },
                link: Some((id, link)),
            },
        );
        drop(hosts);
        if let Some((_, previous)) = previous.and_then(|host| host.link) {
            previous.close();
        }
        self.changes.send_modify(|generation| *generation += 1);
        Ok(id)
    }

    /// Marks `name` offline unless a newer link already replaced link `id`.
    pub(super) fn disconnected(&self, name: &str, id: u64) {
        let mut hosts = self.hosts.lock().unwrap();
        let Some(host) = hosts.get_mut(name) else {
            return;
        };
        if host.link.as_ref().is_none_or(|(current, _)| *current != id) {
            return;
        }
        if let Some((_, link)) = host.link.take() {
            link.close();
        }
        host.status = HostStatus::Offline {
            since: timestamp_ms(),
        };
        drop(hosts);
        self.changes.send_modify(|generation| *generation += 1);
    }

    /// Whether link `id` still serves `name`.
    pub(super) fn is_current(&self, name: &str, id: u64) -> bool {
        self.hosts
            .lock()
            .unwrap()
            .get(name)
            .and_then(|host| host.link.as_ref())
            .is_some_and(|(current, _)| *current == id)
    }

    fn prune(&self) {
        let now = timestamp_ms();
        let mut hosts = self.hosts.lock().unwrap();
        let before = hosts.len();
        hosts.retain(|_, host| match host.status {
            HostStatus::Offline { since } => now.saturating_sub(since) < OFFLINE_RETENTION_MS,
            HostStatus::Online { .. } => true,
        });
        if hosts.len() != before {
            drop(hosts);
            self.changes.send_modify(|generation| *generation += 1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(name: &str, machine: &str) -> HostInfo {
        HostInfo {
            name: name.into(),
            machine_id: machine.into(),
            version: "v".into(),
            kernel: None,
            arch: "x86_64".into(),
        }
    }

    fn link() -> Arc<AgentLink> {
        Arc::new(AgentLink::new().0)
    }

    #[test]
    fn names_are_unique_per_machine_and_reconnects_replace_links() {
        let registry = HostRegistry::new(info("hub", "h"));
        let mut changes = registry.changes();
        assert!(registry.admit(info("hub", "x"), link()).is_err());
        assert!(registry.admit(info("all", "x"), link()).is_err());
        let first = registry.admit(info("b", "m1"), link()).unwrap();
        assert!(changes.has_changed().unwrap());
        changes.mark_unchanged();
        assert!(
            registry
                .admit(info("b", "m2"), link())
                .err()
                .unwrap()
                .contains("another machine")
        );
        let second = registry.admit(info("b", "m1"), link()).unwrap();
        assert!(first != second && !registry.is_current("b", first));
        registry.disconnected("b", first);
        assert!(
            matches!(registry.remote("b"), RemoteHost::Online(_)),
            "a replaced link does not mark the host offline"
        );
        registry.disconnected("b", second);
        assert!(matches!(registry.remote("b"), RemoteHost::Offline));
        assert!(matches!(registry.remote("c"), RemoteHost::Unknown));
        // Another machine may take the name of an offline host.
        registry.admit(info("b", "m2"), link()).unwrap();
        let names: Vec<_> = registry
            .summaries()
            .into_iter()
            .map(|s| (s.name, s.local))
            .collect();
        assert_eq!(names, [("hub".into(), true), ("b".into(), false)]);
        assert_eq!(registry.online().len(), 1);
        for i in 0..HOST_LIMIT - 1 {
            registry.admit(info(&format!("n{i}"), "m"), link()).unwrap();
        }
        assert!(
            registry
                .admit(info("over", "m"), link())
                .err()
                .unwrap()
                .contains("at most")
        );
    }
}
