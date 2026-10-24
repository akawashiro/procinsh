//! HTTP routes for process inspection and system monitoring.
//!
//! # Interface
//!
//! | Definition | Visibility | Signature |
//! | --- | --- | --- |
//! | [`fn@router`] | `pub(super)` | `fn router() -> `[`Router`](axum::Router)`<`[`Arc`](std::sync::Arc)`<`[`AppState`](super::state::AppState)`>>` |
//!
mod process;
mod router;
mod system;
pub(super) use router::router;
