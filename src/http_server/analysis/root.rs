//! Unprivileged interpretation of acquired observations.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`snapshot`] | `pub(crate)` | `mod snapshot` |
//! | [`history`] | `pub(crate)` | `mod history` |
//! | [`hardware`] | `pub(crate)` | `mod hardware` |
//! | [`relations`] | `pub(crate)` | `mod relations` |
//! | [`descriptors`] | `pub(crate)` | `mod descriptors` |
//! | [`activity`] | `pub(crate)` | `mod activity` |
//! | [`details`] | `pub(crate)` | `mod details` |

pub(crate) mod snapshot;

pub(crate) mod history;

pub(crate) mod hardware;
pub(crate) mod relations;

pub(crate) mod descriptors;

pub(crate) mod activity;

pub(crate) mod details;

#[cfg(test)]
mod test_support;
