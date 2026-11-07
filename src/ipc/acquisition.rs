//! Raw process acquisition, bounded samples and sensor diagnostics.
//!
//! # Interface
//! - [`ProcessSummary`] (`pub`): `struct ProcessSummary`.
//! - [`MemoryRollup`] (`pub`): `struct MemoryRollup`.
//! - [`MemoryMaps`] (`pub`): `struct MemoryMaps`.
//! - [`InstructionBytes`] (`pub`): `struct InstructionBytes`.
//! - [`LiveSample`] (`pub`): `struct LiveSample`.
//! - [`ProcessDetailSnapshot`] (`pub`): `struct ProcessDetailSnapshot`.
//! - [`SystemActivity`] (`pub`): `struct SystemActivity`.
//! - [`SystemMonitorStatus`] (`pub`): `struct SystemMonitorStatus`.
//! - [`SensorState`] (`pub`): `enum SensorState`.
//! - [`SampleSource`] (`pub`): `enum SampleSource`.
//! - [`ObservationData`] (`pub`): `struct ObservationData`.
//! - [`EnvironmentData`] (`pub`): `struct EnvironmentData`.
//! - [`AuxData`] (`pub`): `struct AuxData`.
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
#[derive(Clone, Debug)]
pub struct ProcessSummary {
    pub identity: ProcessId,
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
#[derive(Clone, Debug, Default)]
pub struct MemoryRollup {
    pub rss_bytes: Option<u64>,
    pub pss_bytes: Option<u64>,
    pub private_bytes: Option<u64>,
}
pub struct MemoryMaps {
    pub process_id: ProcessId,
    pub maps: Vec<SmapsEntry>,
    pub error: Option<String>,
    pub captured_at: Option<u64>,
    pub rollup: Option<MemoryRollup>,
}
#[derive(Clone, Debug)]
pub struct InstructionBytes {
    pub address: u64,
    pub bytes: Vec<u8>,
    pub error: Option<String>,
}
/// Latest thread sample with loss and error diagnostics.
/// `sampled_at` is Unix milliseconds; `sample_age_ms` uses monotonic time.
#[derive(Clone, Debug)]
pub struct LiveSample {
    pub tid: i32,
    /// Raw executable header; interpreted only by the HTTP server.
    pub executable_header: Option<[u8; 20]>,
    pub sampled_at: Option<u64>,
    pub sample_time_ns: Option<u64>,
    #[cfg(test)]
    pub sample_source: Option<SampleSource>,
    pub sample_age_ms: Option<u64>,
    pub cpu: Option<u32>,
    pub lost_samples: u64,
    pub registers: Vec<Register>,
    pub raw_registers: Option<RegisterSet>,
    pub stack_bytes: Vec<u8>,
    pub map_epoch: Option<u64>,
    pub context: Option<Arc<MappingSnapshot>>,
    pub call_stack: Vec<StackFrame>,
    pub disassembly: Option<InstructionBytes>,
    pub unwind_stop: String,
    pub error: Option<String>,
}
#[derive(Clone, Debug)]
pub struct ProcessDetailSnapshot {
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
#[derive(Clone)]
pub struct SystemActivity {
    pub captured_at: u64,
    pub window_ms: u64,
    pub files: Vec<FileActivity>,
    pub ipc: Vec<IpcActivity>,
    pub cpu: Vec<CpuActivity>,
    pub signals: Vec<SignalEvent>,
    pub status: SystemMonitorStatus,
    pub raw_activity: Option<RawActivity>,
}
#[derive(Clone, Default)]
/// State and collection health of the system monitoring sensors.
pub struct SystemMonitorStatus {
    pub active: bool,
    pub ipc: SensorState,
    pub cpu: SensorState,
    pub files: SensorState,
    pub signals: SensorState,
    pub signals_lost: Option<u64>,
    pub coverage: Option<String>,
    pub files_coverage: Option<String>,
    pub lost: Option<u64>,
    pub unresolved: Option<u64>,
    pub files_lost: Option<u64>,
}
/// Health of an acquisition sensor; unavailable and error states carry a diagnostic.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum SensorState {
    #[default]
    Idle,
    Observing,
    Unavailable(String),
    Error(String),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleSource {
    CpuClock,
    Ptrace,
    ContextSwitch { preempted: bool },
}

#[derive(Clone, Debug)]
pub struct ObservationData {
    pub process_sample: ProcessSample,
    pub threads_only: bool,
}
#[derive(Clone, Debug)]
pub struct EnvironmentData {
    pub process_id: ProcessId,
    pub captured_at: u64,
    pub raw_environment: Vec<u8>,
}
#[derive(Clone, Debug)]
pub struct AuxData {
    pub process_id: ProcessId,
    pub captured_at: u64,
    pub word_bits: usize,
    pub raw_auxv: Vec<u8>,
    pub raw_aux_strings: Vec<(u64, Option<String>, Option<String>)>,
}

crate::ipc::codec::wire_struct!(ProcessSummary {
    identity,
    parent_pid,
    name,
    executable,
    command_line,
    uid,
    username,
    euid,
    effective_username,
    state,
    started_at,
    cpu_percent,
    rss_bytes,
    thread_count,
    raw_ticks,
    raw_measured_ns
});

crate::ipc::codec::wire_struct!(MemoryRollup {
    rss_bytes,
    pss_bytes,
    private_bytes
});

crate::ipc::codec::wire_struct!(MemoryMaps {
    process_id,
    maps,
    error,
    captured_at,
    rollup
});

crate::ipc::codec::wire_struct!(InstructionBytes {
    address,
    bytes,
    error
});

impl crate::ipc::codec::Wire for LiveSample {
    fn write(&self, o: &mut crate::ipc::codec::Encoder) -> anyhow::Result<()> {
        crate::ipc::codec::Wire::write(&self.tid, o)?;
        crate::ipc::codec::Wire::write(&self.executable_header, o)?;
        crate::ipc::codec::Wire::write(&self.sampled_at, o)?;
        crate::ipc::codec::Wire::write(&self.sample_time_ns, o)?;
        crate::ipc::codec::Wire::write(&self.sample_age_ms, o)?;
        crate::ipc::codec::Wire::write(&self.cpu, o)?;
        crate::ipc::codec::Wire::write(&self.lost_samples, o)?;
        crate::ipc::codec::Wire::write(&self.registers, o)?;
        crate::ipc::codec::Wire::write(&self.raw_registers, o)?;
        crate::ipc::codec::Wire::write(&self.stack_bytes, o)?;
        crate::ipc::codec::Wire::write(&self.map_epoch, o)?;
        crate::ipc::codec::Wire::write(&self.call_stack, o)?;
        crate::ipc::codec::Wire::write(&self.disassembly, o)?;
        crate::ipc::codec::Wire::write(&self.unwind_stop, o)?;
        crate::ipc::codec::Wire::write(&self.error, o)?;
        Ok(())
    }
    fn read(i: &mut crate::ipc::codec::Decoder<'_>) -> anyhow::Result<Self> {
        Ok(Self {
            tid: crate::ipc::codec::Wire::read(i)?,
            executable_header: crate::ipc::codec::Wire::read(i)?,
            sampled_at: crate::ipc::codec::Wire::read(i)?,
            sample_time_ns: crate::ipc::codec::Wire::read(i)?,
            sample_age_ms: crate::ipc::codec::Wire::read(i)?,
            cpu: crate::ipc::codec::Wire::read(i)?,
            lost_samples: crate::ipc::codec::Wire::read(i)?,
            registers: crate::ipc::codec::Wire::read(i)?,
            raw_registers: crate::ipc::codec::Wire::read(i)?,
            stack_bytes: crate::ipc::codec::Wire::read(i)?,
            map_epoch: crate::ipc::codec::Wire::read(i)?,
            call_stack: crate::ipc::codec::Wire::read(i)?,
            disassembly: crate::ipc::codec::Wire::read(i)?,
            unwind_stop: crate::ipc::codec::Wire::read(i)?,
            error: crate::ipc::codec::Wire::read(i)?,
            context: Default::default(),
            #[cfg(test)]
            sample_source: Default::default(),
        })
    }
}

impl crate::ipc::codec::Wire for ProcessDetailSnapshot {
    fn write(&self, o: &mut crate::ipc::codec::Encoder) -> anyhow::Result<()> {
        crate::ipc::codec::Wire::write(&self.summary, o)?;
        crate::ipc::codec::Wire::write(&self.exited, o)?;
        crate::ipc::codec::Wire::write(&self.error, o)?;
        crate::ipc::codec::Wire::write(&self.process_sample, o)?;
        crate::ipc::codec::Wire::write(&self.maps, o)?;
        crate::ipc::codec::Wire::write(&self.maps_error, o)?;
        crate::ipc::codec::Wire::write(&self.maps_captured_at, o)?;
        crate::ipc::codec::Wire::write(&self.rollup, o)?;
        crate::ipc::codec::Wire::write(&self.live_samples, o)?;
        crate::ipc::codec::Wire::write(&self.mapping_snapshots, o)?;
        crate::ipc::codec::Wire::write(&self.sampling_error, o)?;
        Ok(())
    }
    fn read(i: &mut crate::ipc::codec::Decoder<'_>) -> anyhow::Result<Self> {
        Ok(Self {
            summary: crate::ipc::codec::Wire::read(i)?,
            exited: crate::ipc::codec::Wire::read(i)?,
            error: crate::ipc::codec::Wire::read(i)?,
            process_sample: crate::ipc::codec::Wire::read(i)?,
            maps: crate::ipc::codec::Wire::read(i)?,
            maps_error: crate::ipc::codec::Wire::read(i)?,
            maps_captured_at: crate::ipc::codec::Wire::read(i)?,
            rollup: crate::ipc::codec::Wire::read(i)?,
            live_samples: crate::ipc::codec::Wire::read(i)?,
            mapping_snapshots: crate::ipc::codec::Wire::read(i)?,
            sampling_error: crate::ipc::codec::Wire::read(i)?,
            #[cfg(test)]
            observation: Default::default(),
            #[cfg(test)]
            history: Default::default(),
        })
    }
}

crate::ipc::codec::wire_struct!(SystemActivity {
    captured_at,
    window_ms,
    files,
    ipc,
    cpu,
    signals,
    status,
    raw_activity
});

crate::ipc::codec::wire_struct!(SystemMonitorStatus {
    active,
    ipc,
    cpu,
    files,
    signals,
    signals_lost,
    coverage,
    files_coverage,
    lost,
    unresolved,
    files_lost
});

crate::ipc::codec::wire_enum!(SensorState{0=>Idle,1=>Observing,2=>Unavailable(value),3=>Error(value)});

crate::ipc::codec::wire_enum!(SampleSource{0=>CpuClock,1=>Ptrace,2=>ContextSwitch{preempted}});

crate::ipc::codec::wire_struct!(ObservationData {
    process_sample,
    threads_only
});

crate::ipc::codec::wire_struct!(EnvironmentData {
    process_id,
    captured_at,
    raw_environment
});

crate::ipc::codec::wire_struct!(AuxData {
    process_id,
    captured_at,
    word_bits,
    raw_auxv,
    raw_aux_strings
});
