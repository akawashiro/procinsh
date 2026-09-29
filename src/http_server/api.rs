//! HTTP routes for process inspection and system monitoring.
//!
//! # Interface
//!
//! Re-export visibility: `pub(super)`. Follow the definition link, then **Source**, for the implementation.
//!
//! | Definition | Signature |
//! | --- | --- |
//! | [`fn@router`] | `fn router() -> Router<Arc<AppState>>` |
//!
//! Types: [`Router`](axum::Router), [`Arc`](std::sync::Arc), [`AppState`](super::state::AppState).
mod process;
mod router;
mod system;
pub(super) use router::router;
