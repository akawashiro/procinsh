//! Sysfs location used by protected neighbor acquisition.
//!
//! # Interface
//! [`HardwareSources`] (`pub(super)`): `struct HardwareSources` supplies the sysfs root.
#[cfg(not(test))]
pub(super) struct HardwareSources {
    pub sys: std::path::PathBuf,
    pub proc: std::path::PathBuf,
}
#[cfg(not(test))]
impl Default for HardwareSources {
    fn default() -> Self {
        Self {
            sys: "/sys".into(),
            proc: "/proc".into(),
        }
    }
}
