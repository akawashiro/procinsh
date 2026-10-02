//! Configure each sensor before attaching any hooks.
use anyhow::{Context, Result};
use libbpf_rs::{MapCore, MapFlags};
use std::os::unix::fs::MetadataExt;

#[derive(Clone, Copy)]
pub(super) struct PidNamespace(u64);

impl PidNamespace {
    pub(super) fn current() -> Result<Self> {
        Ok(Self(
            std::fs::metadata("/proc/self/ns/pid")
                .context("read observer PID namespace")?
                .ino(),
        ))
    }

    pub(super) fn configure(self, object: &libbpf_rs::Object) -> Result<()> {
        object
            .maps()
            .find(|map| map.name() == "pid_namespace")
            .context("pid_namespace map")?
            .update(&0u32.to_ne_bytes(), &self.0.to_ne_bytes(), MapFlags::ANY)
            .context("configure observer PID namespace")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn current_namespace_matches_procfs() {
        let ns = PidNamespace::current().unwrap();
        assert!(ns.0 > 0);
        assert_eq!(
            std::fs::read_link("/proc/self/ns/pid")
                .unwrap()
                .to_str()
                .unwrap(),
            format!("pid:[{}]", ns.0)
        );
    }
}
