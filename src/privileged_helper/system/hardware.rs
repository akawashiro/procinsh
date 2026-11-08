//! Sysfs location used by protected neighbor acquisition.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`HardwareSources`] | `pub(super)` | `struct HardwareSources` |

#[cfg(not(test))]
pub(super) struct HardwareSources {
    pub(super) sys: std::path::PathBuf,
    pub(super) proc: std::path::PathBuf,
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
