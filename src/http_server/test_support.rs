use std::{process::Command, sync::OnceLock};
pub(super) fn build_targets() {
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
