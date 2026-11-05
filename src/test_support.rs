//! Shared fixture compilation for tests of acquisition and analysis.

//!
//! # Interface
//! Test-only helpers; this module is absent from normal builds.

use std::{process::Command, sync::OnceLock};

pub(crate) fn build_targets() {
    static BUILD: OnceLock<()> = OnceLock::new();
    BUILD.get_or_init(|| {
        assert!(
            Command::new("sh")
                .arg("tests/targets/build.sh")
                .status()
                .unwrap()
                .success()
        );
    });
}
