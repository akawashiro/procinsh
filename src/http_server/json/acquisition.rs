//! Main-only JSON representation of native acquisition values.
//!
//! # Interface
//!
//! No named types or functions are exported.
//!
//! Serde implementations for IPC acquisition types; no named types or functions are exported.

use crate::ipc::acquisition::*;

fn serialize_architecture<S: serde::Serializer>(
    header: &Option<[u8; 20]>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    use crate::http_server::analysis::snapshot::architecture::architecture_from_elf_header;

    let architecture = header.as_ref().and_then(architecture_from_elf_header);
    serde::Serialize::serialize(&architecture, serializer)
}

use crate::ipc::{
    activity::{CpuActivity, FileActivity, IpcActivity, RawActivity, SignalEvent},
    model::{
        memory::{MappingSnapshot, SmapsEntry},
        process::ProcessSample,
    },
    registers::{Register, RegisterSet},
    schema::ProcessId,
    stack::StackFrame,
};
#[cfg(test)]
use crate::ipc::{history::HistoryPoint, model::process::ProcessObservation};
#[cfg(test)]
use std::collections::VecDeque;
use std::sync::Arc;
#[derive(serde::Serialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::acquisition::ProcessSummary")]
struct ProcessSummaryJson {
    identity: ProcessId,
    #[serde(skip)]
    parent_pid: i32,
    name: String,
    executable: Option<String>,
    command_line: Option<Vec<String>>,
    uid: Option<u32>,
    username: Option<String>,
    euid: Option<u32>,
    effective_username: Option<String>,
    state: String,
    /// Unix milliseconds derived from boot time and process start ticks; absent if unavailable.
    started_at: Option<u64>,
    cpu_percent: Option<f64>,
    rss_bytes: u64,
    thread_count: u32,
    raw_ticks: u64,
    raw_measured_ns: u64,
}
impl serde::Serialize for crate::ipc::acquisition::ProcessSummary {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        ProcessSummaryJson::serialize(self, s)
    }
}

#[derive(serde::Serialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::acquisition::MemoryRollup")]
struct MemoryRollupJson {
    rss_bytes: Option<u64>,
    pss_bytes: Option<u64>,
    private_bytes: Option<u64>,
}
impl serde::Serialize for crate::ipc::acquisition::MemoryRollup {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        MemoryRollupJson::serialize(self, s)
    }
}

#[derive(serde::Serialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::acquisition::MemoryMaps")]
struct MemoryMapsJson {
    process_id: ProcessId,
    maps: Vec<SmapsEntry>,
    error: Option<String>,
    captured_at: Option<u64>,
    rollup: Option<MemoryRollup>,
}
impl serde::Serialize for crate::ipc::acquisition::MemoryMaps {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        MemoryMapsJson::serialize(self, s)
    }
}

#[derive(serde::Serialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::acquisition::InstructionBytes")]
struct InstructionBytesJson {
    #[serde(serialize_with = "crate::http_server::model::memory::hex")]
    address: u64,
    bytes: Vec<u8>,
    error: Option<String>,
}
impl serde::Serialize for crate::ipc::acquisition::InstructionBytes {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        InstructionBytesJson::serialize(self, s)
    }
}

#[derive(serde::Serialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::acquisition::LiveSample")]
struct LiveSampleJson {
    tid: i32,
    #[serde(rename = "architecture", serialize_with = "serialize_architecture")]
    executable_header: Option<[u8; 20]>,
    sampled_at: Option<u64>,
    sample_time_ns: Option<u64>,
    #[cfg(test)]
    #[serde(skip)]
    sample_source: Option<SampleSource>,
    sample_age_ms: Option<u64>,
    cpu: Option<u32>,
    lost_samples: u64,
    registers: Vec<Register>,
    raw_registers: Option<RegisterSet>,
    stack_bytes: Vec<u8>,
    map_epoch: Option<u64>,
    #[serde(skip)]
    context: Option<Arc<MappingSnapshot>>,
    call_stack: Vec<StackFrame>,
    disassembly: Option<InstructionBytes>,
    unwind_stop: String,
    error: Option<String>,
}
impl serde::Serialize for crate::ipc::acquisition::LiveSample {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        LiveSampleJson::serialize(self, s)
    }
}

#[derive(serde::Serialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::acquisition::ProcessDetailSnapshot")]
struct ProcessDetailSnapshotJson {
    summary: ProcessSummary,
    exited: bool,
    error: Option<String>,
    #[cfg(test)]
    observation: Option<ProcessObservation>,
    process_sample: ProcessSample,
    #[cfg(test)]
    history: VecDeque<HistoryPoint>,
    maps: Vec<SmapsEntry>,
    maps_error: Option<String>,
    maps_captured_at: Option<u64>,
    rollup: Option<MemoryRollup>,
    live_samples: Vec<LiveSample>,
    mapping_snapshots: Vec<MappingSnapshot>,
    sampling_error: Option<String>,
}
impl serde::Serialize for crate::ipc::acquisition::ProcessDetailSnapshot {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        ProcessDetailSnapshotJson::serialize(self, s)
    }
}

#[derive(serde::Serialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::acquisition::SystemActivity")]
struct SystemActivityJson {
    captured_at: u64,
    window_ms: u64,
    files: Vec<FileActivity>,
    ipc: Vec<IpcActivity>,
    cpu: Vec<CpuActivity>,
    signals: Vec<SignalEvent>,
    status: SystemMonitorStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    raw_activity: Option<RawActivity>,
}
impl serde::Serialize for crate::ipc::acquisition::SystemActivity {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        SystemActivityJson::serialize(self, s)
    }
}

#[derive(serde::Serialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::acquisition::SystemMonitorStatus")]
/// State and collection health of the system monitoring sensors.
struct SystemMonitorStatusJson {
    active: bool,
    ipc: SensorState,
    cpu: SensorState,
    files: SensorState,
    signals: SensorState,
    #[serde(skip_serializing_if = "Option::is_none")]
    signals_lost: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    coverage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    files_coverage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    lost: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    unresolved: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    files_lost: Option<u64>,
}
impl serde::Serialize for crate::ipc::acquisition::SystemMonitorStatus {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        SystemMonitorStatusJson::serialize(self, s)
    }
}

#[derive(serde::Serialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::acquisition::SensorState")]
#[serde(tag = "state", content = "message", rename_all = "snake_case")]
enum SensorStateJson {
    Idle,
    Observing,
    Unavailable(String),
    Error(String),
}
impl serde::Serialize for crate::ipc::acquisition::SensorState {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        SensorStateJson::serialize(self, s)
    }
}

impl serde::Serialize for ObservationData {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let observation =
            crate::http_server::model::process::initial_observation(&self.process_sample);
        if self.threads_only {
            serde::Serialize::serialize(&observation.threads, s)
        } else {
            serde::Serialize::serialize(&observation, s)
        }
    }
}
impl serde::Serialize for EnvironmentData {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut value = serde_json::json!({"process_id":self.process_id,"captured_at":self.captured_at,"raw_environment":self.raw_environment});
        crate::http_server::analysis::details::format_process_metadata(&mut value)
            .map_err(serde::ser::Error::custom)?;
        serde::Serialize::serialize(&value, s)
    }
}
impl serde::Serialize for AuxData {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut value = serde_json::json!({"process_id":self.process_id,"captured_at":self.captured_at,"word_bits":self.word_bits,"raw_auxv":self.raw_auxv,"raw_aux_strings":self.raw_aux_strings});
        crate::http_server::analysis::details::format_process_metadata(&mut value)
            .map_err(serde::ser::Error::custom)?;
        serde::Serialize::serialize(&value, s)
    }
}
