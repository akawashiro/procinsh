//! Hosts known to a hub: the local machine and remote agents by name.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`HostSummary`] | `pub(in crate::http_server)` | `struct HostSummary` |
//! | [`RemoteHost`] | `pub(in crate::http_server)` | `enum RemoteHost` |
//! | [`HostRegistry`] | `pub(in crate::http_server)` | `struct HostRegistry` |
//! | [`create_host_registry`] | `pub(in crate::http_server)` | `fn create_host_registry(local: `[`HostInfo`]`) -> `[`HostRegistry`] |
//! | [`local_host_registry`] | `pub(in crate::http_server)` | `fn local_host_registry(state: &`[`HostRegistry`]`) -> &`[`HostInfo`] |
//! | [`remote_host_registry`] | `pub(in crate::http_server)` | `fn remote_host_registry(state: &`[`HostRegistry`]`, name: &str) -> `[`RemoteHost`] |
//! | [`online_host_registry`] | `pub(in crate::http_server)` | `fn online_host_registry( state: &`[`HostRegistry`]`, ) -> `[`Vec`]`<(`[`String`]`, `[`Arc`]`<`[`AgentLink`]`>)>` |
//! | [`changes_host_registry`] | `pub(in crate::http_server)` | `fn changes_host_registry(state: &`[`HostRegistry`]`) -> `[`watch::Receiver`]`<u64>` |
//! | [`summaries_host_registry`] | `pub(in crate::http_server)` | `fn summaries_host_registry(state: &`[`HostRegistry`]`) -> `[`Vec`]`<`[`HostSummary`]`>` |
//! | [`admit_host_registry`] | `pub(super)` | `fn admit_host_registry( state: &`[`HostRegistry`]`, info: `[`HostInfo`]`, link: `[`Arc`]`<`[`AgentLink`]`>, ) -> `[`Result`](anyhow::Result)`<u64, `[`String`]`>` |
//! | [`disconnected_host_registry`] | `pub(super)` | `fn disconnected_host_registry(state: &`[`HostRegistry`]`, name: &str, id: u64)` |
//! | [`host_registry_is_current`] | `pub(super)` | `fn host_registry_is_current(state: &`[`HostRegistry`]`, name: &str, id: u64) -> bool` |

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
enum HostStatus {
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
    name: String,
    local: bool,
    status: HostStatus,
    pub(super) machine_id: String,
    version: String,
    kernel: Option<String>,
    arch: String,
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

pub(in crate::http_server) fn create_host_registry(local: HostInfo) -> HostRegistry {
    HostRegistry {
        local,
        started: timestamp_ms(),
        hosts: Mutex::new(BTreeMap::new()),
        links: AtomicU64::new(0),
        changes: watch::channel(0).0,
    }
}

pub(in crate::http_server) fn local_host_registry(state: &HostRegistry) -> &HostInfo {
    &state.local
}

pub(in crate::http_server) fn remote_host_registry(state: &HostRegistry, name: &str) -> RemoteHost {
    match state.hosts.lock().unwrap().get(name) {
        Some(Remote {
            link: Some((_, link)),
            ..
        }) => RemoteHost::Online(link.clone()),
        Some(_) => RemoteHost::Offline,
        None => RemoteHost::Unknown,
    }
}

/// Online remote hosts in name order.
pub(in crate::http_server) fn online_host_registry(
    state: &HostRegistry,
) -> Vec<(String, Arc<AgentLink>)> {
    state
        .hosts
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(name, host)| Some((name.clone(), host.link.as_ref()?.1.clone())))
        .collect()
}

pub(in crate::http_server) fn changes_host_registry(state: &HostRegistry) -> watch::Receiver<u64> {
    state.changes.subscribe()
}

/// The local host first, then remote hosts in name order.
pub(in crate::http_server) fn summaries_host_registry(state: &HostRegistry) -> Vec<HostSummary> {
    prune_host_registry(state);
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
        &state.local,
        true,
        HostStatus::Online {
            since: state.started,
        },
    )];
    result.extend(
        state
            .hosts
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
pub(super) fn admit_host_registry(
    state: &HostRegistry,
    info: HostInfo,
    link: Arc<AgentLink>,
) -> Result<u64, String> {
    use crate::http_server::hosts::link::close_agent_link;

    if !valid_host_name(&info.name) {
        return Err(format!("invalid host name {:?}", info.name));
    }
    if info.name == state.local.name {
        return Err(format!("host name {} is the hub's own name", info.name));
    }
    prune_host_registry(state);
    let id = state.links.fetch_add(1, Ordering::Relaxed) + 1;
    let mut hosts = state.hosts.lock().unwrap();
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
        close_agent_link(&previous);
    }
    state.changes.send_modify(|generation| *generation += 1);
    Ok(id)
}

/// Marks `name` offline unless a newer link already replaced link `id`.
pub(super) fn disconnected_host_registry(state: &HostRegistry, name: &str, id: u64) {
    use crate::http_server::hosts::link::close_agent_link;

    let mut hosts = state.hosts.lock().unwrap();
    let Some(host) = hosts.get_mut(name) else {
        return;
    };
    if host.link.as_ref().is_none_or(|(current, _)| *current != id) {
        return;
    }
    if let Some((_, link)) = host.link.take() {
        close_agent_link(&link);
    }
    host.status = HostStatus::Offline {
        since: timestamp_ms(),
    };
    drop(hosts);
    state.changes.send_modify(|generation| *generation += 1);
}

/// Whether link `id` still serves `name`.
pub(super) fn host_registry_is_current(state: &HostRegistry, name: &str, id: u64) -> bool {
    state
        .hosts
        .lock()
        .unwrap()
        .get(name)
        .and_then(|host| host.link.as_ref())
        .is_some_and(|(current, _)| *current == id)
}

fn prune_host_registry(state: &HostRegistry) {
    let now = timestamp_ms();
    let mut hosts = state.hosts.lock().unwrap();
    let before = hosts.len();
    hosts.retain(|_, host| match host.status {
        HostStatus::Offline { since } => now.saturating_sub(since) < OFFLINE_RETENTION_MS,
        HostStatus::Online { .. } => true,
    });
    if hosts.len() != before {
        drop(hosts);
        state.changes.send_modify(|generation| *generation += 1);
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

    fn agent_link_fixture() -> Arc<AgentLink> {
        use crate::http_server::hosts::link::create_agent_link;

        Arc::new(create_agent_link().0)
    }

    #[test]
    fn names_are_unique_per_machine_and_reconnects_replace_links() {
        let registry = super::create_host_registry(info("hub", "h"));
        let mut changes = super::changes_host_registry(&registry);
        assert!(
            super::admit_host_registry(&registry, info("hub", "x"), agent_link_fixture()).is_err()
        );
        assert!(
            super::admit_host_registry(&registry, info("all", "x"), agent_link_fixture()).is_err()
        );
        let first =
            super::admit_host_registry(&registry, info("b", "m1"), agent_link_fixture()).unwrap();
        assert!(changes.has_changed().unwrap());
        changes.mark_unchanged();
        assert!(
            super::admit_host_registry(&registry, info("b", "m2"), agent_link_fixture())
                .err()
                .unwrap()
                .contains("another machine")
        );
        let second =
            super::admit_host_registry(&registry, info("b", "m1"), agent_link_fixture()).unwrap();
        assert!(first != second && !super::host_registry_is_current(&registry, "b", first));
        super::disconnected_host_registry(&registry, "b", first);
        assert!(
            matches!(
                super::remote_host_registry(&registry, "b"),
                RemoteHost::Online(_)
            ),
            "a replaced link does not mark the host offline"
        );
        super::disconnected_host_registry(&registry, "b", second);
        assert!(matches!(
            super::remote_host_registry(&registry, "b"),
            RemoteHost::Offline
        ));
        assert!(matches!(
            super::remote_host_registry(&registry, "c"),
            RemoteHost::Unknown
        ));
        // Another machine may take the name of an offline host.
        super::admit_host_registry(&registry, info("b", "m2"), agent_link_fixture()).unwrap();
        let names: Vec<_> = super::summaries_host_registry(&registry)
            .into_iter()
            .map(|s| (s.name, s.local))
            .collect();
        assert_eq!(names, [("hub".into(), true), ("b".into(), false)]);
        assert_eq!(super::online_host_registry(&registry).len(), 1);
        for i in 0..HOST_LIMIT - 1 {
            super::admit_host_registry(
                &registry,
                info(&format!("n{i}"), "m"),
                agent_link_fixture(),
            )
            .unwrap();
        }
        assert!(
            super::admit_host_registry(&registry, info("over", "m"), agent_link_fixture())
                .err()
                .unwrap()
                .contains("at most")
        );
    }
}
