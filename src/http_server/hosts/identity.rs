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
