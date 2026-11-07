//! Main-only JSON representation of native acquisition values.
//!
//! # Interface
//! Serde implementations for IPC acquisition types; no named types or functions are exported.
use crate::ipc::acquisition::*;

fn serialize_architecture<S: serde::Serializer>(
    header: &Option<[u8; 20]>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let architecture = header.as_ref().and_then(
        crate::http_server::analysis::snapshot::architecture::Architecture::from_elf_header,
    );
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
    pub identity: ProcessId,
    #[serde(skip)]
    pub parent_pid: i32,
    pub name: String,
    pub executable: Option<String>,
    pub command_line: Option<Vec<String>>,
    pub uid: Option<u32>,
    pub username: Option<String>,
    pub euid: Option<u32>,
    pub effective_username: Option<String>,
    pub state: String,
    /// Unix milliseconds derived from boot time and process start ticks; absent if unavailable.
    pub started_at: Option<u64>,
    pub cpu_percent: Option<f64>,
    pub rss_bytes: u64,
    pub thread_count: u32,
    pub raw_ticks: u64,
    pub raw_measured_ns: u64,
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
    pub rss_bytes: Option<u64>,
    pub pss_bytes: Option<u64>,
    pub private_bytes: Option<u64>,
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
    pub process_id: ProcessId,
    pub maps: Vec<SmapsEntry>,
    pub error: Option<String>,
    pub captured_at: Option<u64>,
    pub rollup: Option<MemoryRollup>,
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
    pub address: u64,
    pub bytes: Vec<u8>,
    pub error: Option<String>,
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
    pub tid: i32,
    #[serde(rename = "architecture", serialize_with = "serialize_architecture")]
    pub executable_header: Option<[u8; 20]>,
    pub sampled_at: Option<u64>,
    pub sample_time_ns: Option<u64>,
    #[cfg(test)]
    #[serde(skip)]
    pub sample_source: Option<SampleSource>,
    pub sample_age_ms: Option<u64>,
    pub cpu: Option<u32>,
    pub lost_samples: u64,
    pub registers: Vec<Register>,
    pub raw_registers: Option<RegisterSet>,
    pub stack_bytes: Vec<u8>,
    pub map_epoch: Option<u64>,
    #[serde(skip)]
    pub context: Option<Arc<MappingSnapshot>>,
    pub call_stack: Vec<StackFrame>,
    pub disassembly: Option<InstructionBytes>,
    pub unwind_stop: String,
    pub error: Option<String>,
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
    pub summary: ProcessSummary,
    pub exited: bool,
    pub error: Option<String>,
    #[cfg(test)]
    pub observation: Option<ProcessObservation>,
    pub process_sample: ProcessSample,
    #[cfg(test)]
    pub history: VecDeque<HistoryPoint>,
    pub maps: Vec<SmapsEntry>,
    pub maps_error: Option<String>,
    pub maps_captured_at: Option<u64>,
    pub rollup: Option<MemoryRollup>,
    pub live_samples: Vec<LiveSample>,
    pub mapping_snapshots: Vec<MappingSnapshot>,
    pub sampling_error: Option<String>,
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
    pub captured_at: u64,
    pub window_ms: u64,
    pub files: Vec<FileActivity>,
    pub ipc: Vec<IpcActivity>,
    pub cpu: Vec<CpuActivity>,
    pub signals: Vec<SignalEvent>,
    pub status: SystemMonitorStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_activity: Option<RawActivity>,
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
    pub active: bool,
    pub ipc: SensorState,
    pub cpu: SensorState,
    pub files: SensorState,
    pub signals: SensorState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signals_lost: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub coverage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files_coverage: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lost: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unresolved: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files_lost: Option<u64>,
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
