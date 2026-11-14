//! Initial and ten-second ptrace snapshots, non-stopping perf samples,
//! raw register/stack snapshots and best-effort instruction-byte capture.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`bootstrap_sampler`] | `pub(in crate::privileged_helper::process)` | `fn bootstrap_sampler( state: &mut `[`Sampler`](capture::Sampler)`, maps: &[`[`MemoryMap`](crate::ipc::model::memory::MemoryMap)`], ) -> `[`Result`](anyhow::Result)`<()>` |
//! | [`create_sampler`] | `pub(in crate::privileged_helper::process)` | `fn create_sampler(id: `[`ProcessId`](crate::ipc::schema::ProcessId)`) -> `[`Sampler`](capture::Sampler) |
//! | [`latest_sampler`] | `pub(in crate::privileged_helper::process)` | `fn latest_sampler(state: &`[`Sampler`](capture::Sampler)`) -> `[`Vec`]`<`[`LiveSample`](crate::ipc::acquisition::LiveSample)`>` |
//! | [`mapping_snapshots_sampler`] | `pub(in crate::privileged_helper::process)` | `fn mapping_snapshots_sampler( state: &`[`Sampler`](capture::Sampler)`, ) -> `[`Vec`]`<`[`MappingSnapshot`](crate::ipc::model::memory::MappingSnapshot)`>` |
//! | [`poll_sampler`] | `pub(in crate::privileged_helper::process)` | `fn poll_sampler( state: &mut `[`Sampler`](capture::Sampler)`, maps: &[`[`MemoryMap`](crate::ipc::model::memory::MemoryMap)`], ) -> `[`Result`](anyhow::Result)`<()>` |

mod arch;
mod capture;
mod instruction_bytes;
mod perf;
mod ptrace;
mod sample;

pub(in crate::privileged_helper::process) use capture::{
    bootstrap_sampler, create_sampler, latest_sampler, mapping_snapshots_sampler, poll_sampler,
};
