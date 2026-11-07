//! Acquire bounded environment bytes after checking privacy and process identity.
//!
//! # Interface
//! - [`read_environment`] (`pub(super)`): `fn read_environment(id: ProcessId) -> anyhow::Result<EnvironmentData>`.
//!
//! Process identities use [`ProcessId`]; captured bytes use [`EnvironmentData`](crate::ipc::acquisition::EnvironmentData).

use super::procfs::read_bounded_bytes;
use anyhow::{Result, ensure};
use std::fs::File;
use {
    super::{memory, permission_help, timestamp_ms, validate_process_identity},
    crate::ipc::schema::ProcessId,
};

const MAX_ENVIRONMENT: usize = 1024 * 1024;

pub(super) fn read_environment(id: ProcessId) -> Result<crate::ipc::acquisition::EnvironmentData> {
    ensure!(
        !memory::privacy_enabled(),
        "Environment variables are not read in privacy mode."
    );
    validate_process_identity(id)?;
    let bytes = read_bounded_bytes(
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
