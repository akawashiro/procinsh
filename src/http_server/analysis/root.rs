//! Unprivileged interpretation of acquired observations.

//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`snapshot`] | `pub` | `mod snapshot` |
//! | [`history`] | `pub` | `mod history` |
//! | [`hardware`] | `pub` | `mod hardware` |
//! | [`relations`] | `pub` | `mod relations` |
//! | [`descriptors`] | `pub` | `mod descriptors` |
//! | [`activity`] | `pub` | `mod activity` |
//! | [`details`] | `pub` | `mod details` |

pub mod snapshot;

pub mod history;

pub mod hardware;
pub mod relations;

pub mod descriptors;

pub mod activity;

pub mod details;

#[cfg(test)]
mod test_support;
