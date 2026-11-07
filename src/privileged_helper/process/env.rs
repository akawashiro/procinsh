//! Acquire bounded environment bytes after checking privacy and process identity.
//!
//! # Interface
//! - [`environment`] (`pub(super)`): `fn environment(id: ProcessId) -> anyhow::Result<EnvironmentData>`.
//!
//! Process identities use [`ProcessId`]; captured bytes use [`EnvironmentData`](crate::ipc::acquisition::EnvironmentData).

use super::procfs::read_limited;
use super::{ProcessId, memory, permission_help, timestamp_ms, validate_process_identity};
use anyhow::{Result, ensure};
use std::fs::File;

const MAX_ENVIRONMENT: usize = 1024 * 1024;

pub(super) fn environment(id: ProcessId) -> Result<crate::ipc::acquisition::EnvironmentData> {
    ensure!(
        !memory::privacy(),
        "Environment variables are not read in privacy mode."
    );
    validate_process_identity(id)?;
    let bytes = read_limited(
        File::open(format!("/proc/{}/environ", id.pid))?,
        MAX_ENVIRONMENT,
    )
    .map_err(|e| anyhow::anyhow!(permission_help("/proc/PID/environ", e)))?;
    validate_process_identity(id)?;
    Ok(crate::ipc::acquisition::EnvironmentData {
        process_id: id,
        captured_at: timestamp_ms(),
        raw_environment: bytes,
    })
}
