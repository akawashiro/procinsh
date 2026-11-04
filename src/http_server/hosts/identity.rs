//! The identity a host advertises to a hub and shows in the host list.

use serde::Serialize;
use std::fs;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(in crate::http_server) struct HostInfo {
    pub(in crate::http_server) name: String,
    /// `/etc/machine-id`, or the boot ID when the machine ID is unavailable.
    pub(in crate::http_server) machine_id: String,
    pub(in crate::http_server) version: String,
    pub(in crate::http_server) kernel: Option<String>,
    /// `std::env::consts::ARCH` of the procinsh build, such as `x86_64`.
    pub(in crate::http_server) arch: String,
}

fn text(path: &str) -> Option<String> {
    let value = fs::read_to_string(path).ok()?;
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// The `?host=` value of the merged view.
pub(in crate::http_server) const ALL_HOSTS: &str = "all";

/// Host names are short, URL-safe labels; [`ALL_HOSTS`] names the merged view.
pub(crate) fn valid_host_name(name: &str) -> bool {
    (1..=63).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        && name != ALL_HOSTS
}

impl HostInfo {
    /// Replace identifying metadata before a remote host enters the registry.
    /// The resulting name is also the routing key for requests and SSE events.
    pub(super) fn protect_remote(&mut self) {
        if crate::http_server::privacy::enabled() {
            self.mask_identity(crate::http_server::privacy::pseudonym);
        }
    }

    fn mask_identity(&mut self, mask: impl Fn(&str) -> String) {
        self.name = format!("host-{}", mask(&self.name).replace('░', ""));
        self.machine_id = mask(&self.machine_id).replace('░', "");
    }

    /// This machine, named by `name` or the kernel host name. In privacy mode
    /// the default name and the machine ID are pseudonyms for this run.
    pub(in crate::http_server) fn local(name: Option<String>) -> Self {
        let machine_id = text("/etc/machine-id")
            .or_else(|| text("/proc/sys/kernel/random/boot_id"))
            .unwrap_or_default();
        let private = crate::http_server::privacy::enabled();
        let pseudonym = || crate::http_server::privacy::pseudonym(&machine_id).replace('░', "");
        Self {
            name: name
                .or_else(|| {
                    if private {
                        Some(format!("host-{}", pseudonym()))
                    } else {
                        text("/proc/sys/kernel/hostname")
                    }
                })
                .unwrap_or_else(|| "localhost".into()),
            machine_id: if private { pseudonym() } else { machine_id },
            version: env!("CARGO_PKG_VERSION").into(),
            kernel: text("/proc/sys/kernel/osrelease"),
            arch: std::env::consts::ARCH.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_names_are_short_url_safe_and_not_all() {
        for name in ["node-a", "gpu01.cluster", "a_b", &"x".repeat(63)] {
            assert!(valid_host_name(name), "{name}");
        }
        for name in ["", "all", "a b", "a/b", "a?b", &"x".repeat(64)] {
            assert!(!valid_host_name(name), "{name}");
        }
        let local = HostInfo::local(Some("override".into()));
        assert_eq!(local.name, "override");
        assert_eq!(local.version, env!("CARGO_PKG_VERSION"));
        assert!(!HostInfo::local(None).name.is_empty());
    }
}

#[cfg(test)]
mod privacy_tests {
    use super::*;

    #[test]
    fn remote_identity_is_private_and_remains_routable() {
        let mut info = HostInfo {
            name: "private-agent".into(),
            machine_id: "private-machine-id".into(),
            version: "v1".into(),
            kernel: None,
            arch: "aarch64".into(),
        };
        info.mask_identity(|value| match value {
            "private-agent" => "░12345678░".into(),
            "private-machine-id" => "░abcdef01░".into(),
            _ => panic!("unexpected identity"),
        });
        assert_eq!(info.name, "host-12345678");
        assert!(valid_host_name(&info.name));
        assert_eq!(info.machine_id, "abcdef01");
        assert_eq!(info.version, "v1");
        let json = serde_json::to_string(&info).unwrap();
        assert!(!json.contains("private-agent"));
        assert!(!json.contains("private-machine-id"));
        let registry =
            super::super::registry::HostRegistry::new(HostInfo::local(Some("hub".into())));
        let link = std::sync::Arc::new(super::super::link::AgentLink::new().0);
        let id = registry.admit(info.clone(), link).unwrap();
        assert!(matches!(
            registry.remote(&info.name),
            super::super::registry::RemoteHost::Online(_)
        ));
        assert_eq!(registry.online()[0].0, info.name);
        assert_eq!(registry.summaries()[1].machine_id, info.machine_id);
        assert!(registry.is_current(&info.name, id));
        registry.disconnected(&info.name, id);
        assert!(matches!(
            registry.remote(&info.name),
            super::super::registry::RemoteHost::Offline
        ));
    }
}
