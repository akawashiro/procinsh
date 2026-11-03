//! Process history values used by acquisition tests.

//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`HistoryPoint`] | `pub(super)` | `struct HistoryPoint` |
//! | [`push`] | `pub(super)` | `fn push(history: &mut VecDeque<HistoryPoint>, observation: &ProcessObservation)` |

pub(super) use crate::http_server::analysis::history::*;
