//! Raw process acquisition, bounded samples and sensor diagnostics.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`ProcessSummary`] | `pub(crate)` | `struct ProcessSummary` |
//! | [`MemoryRollup`] | `pub(crate)` | `struct MemoryRollup` |
//! | [`MemoryMaps`] | `pub(crate)` | `struct MemoryMaps` |
//! | [`InstructionBytes`] | `pub(crate)` | `struct InstructionBytes` |
//! | [`LiveSample`] | `pub(crate)` | `struct LiveSample` |
//! | [`ProcessDetailSnapshot`] | `pub(crate)` | `struct ProcessDetailSnapshot` |
//! | [`SystemActivity`] | `pub(crate)` | `struct SystemActivity` |
//! | [`SystemMonitorStatus`] | `pub(crate)` | `struct SystemMonitorStatus` |
//! | [`SensorState`] | `pub(crate)` | `enum SensorState` |
//! | [`SampleSource`] | `pub(crate)` | `enum SampleSource` |
//! | [`ObservationData`] | `pub(crate)` | `struct ObservationData` |
//! | [`EnvironmentData`] | `pub(crate)` | `struct EnvironmentData` |
//! | [`AuxData`] | `pub(crate)` | `struct AuxData` |

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
pub(crate) struct ProcessSummary {
    pub(crate) identity: ProcessId,
    pub(crate) parent_pid: i32,
    pub(crate) name: String,
    pub(crate) executable: Option<String>,
    pub(crate) command_line: Option<Vec<String>>,
    pub(crate) uid: Option<u32>,
    pub(crate) username: Option<String>,
    pub(crate) euid: Option<u32>,
    pub(crate) effective_username: Option<String>,
    pub(crate) state: String,
    /// Unix milliseconds derived from boot time and process start ticks; absent if unavailable.
    pub(crate) started_at: Option<u64>,
    pub(crate) cpu_percent: Option<f64>,
    pub(crate) rss_bytes: u64,
    pub(crate) thread_count: u32,
    pub(crate) raw_ticks: u64,
    pub(crate) raw_measured_ns: u64,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct MemoryRollup {
    pub(crate) rss_bytes: Option<u64>,
    pub(crate) pss_bytes: Option<u64>,
    pub(crate) private_bytes: Option<u64>,
}
pub(crate) struct MemoryMaps {
    pub(crate) process_id: ProcessId,
    pub(crate) maps: Vec<SmapsEntry>,
    pub(crate) error: Option<String>,
    pub(crate) captured_at: Option<u64>,
    pub(crate) rollup: Option<MemoryRollup>,
}
#[derive(Clone, Debug)]
pub(crate) struct InstructionBytes {
    pub(crate) address: u64,
    pub(crate) bytes: Vec<u8>,
    pub(crate) error: Option<String>,
}
/// Latest thread sample with loss and error diagnostics.
/// `sampled_at` is Unix milliseconds; `sample_age_ms` uses monotonic time.
#[derive(Clone, Debug)]
pub(crate) struct LiveSample {
    pub(crate) tid: i32,
    /// Raw executable header; interpreted only by the HTTP server.
    pub(crate) executable_header: Option<[u8; 20]>,
    pub(crate) sampled_at: Option<u64>,
    pub(crate) sample_time_ns: Option<u64>,
    #[cfg(test)]
    pub(crate) sample_source: Option<SampleSource>,
    pub(crate) sample_age_ms: Option<u64>,
    pub(crate) cpu: Option<u32>,
    pub(crate) lost_samples: u64,
    pub(crate) registers: Vec<Register>,
    pub(crate) raw_registers: Option<RegisterSet>,
    pub(crate) stack_bytes: Vec<u8>,
    pub(crate) map_epoch: Option<u64>,
    pub(crate) context: Option<Arc<MappingSnapshot>>,
    pub(crate) call_stack: Vec<StackFrame>,
    pub(crate) disassembly: Option<InstructionBytes>,
    pub(crate) unwind_stop: String,
    pub(crate) error: Option<String>,
}
/// SSE observation state with detailed maps and independently aged live samples.
/// Map entries flatten structural mappings with optional RSS/PSS measurements.
#[derive(Clone, Debug)]
pub(crate) struct ProcessDetailSnapshot {
    pub(crate) summary: ProcessSummary,
    pub(crate) exited: bool,
    pub(crate) error: Option<String>,
    #[cfg(test)]
    pub(crate) observation: Option<ProcessObservation>,
    pub(crate) process_sample: ProcessSample,
    #[cfg(test)]
    pub(crate) history: VecDeque<HistoryPoint>,
    pub(crate) maps: Vec<SmapsEntry>,
    pub(crate) maps_error: Option<String>,
    pub(crate) maps_captured_at: Option<u64>,
    pub(crate) rollup: Option<MemoryRollup>,
    pub(crate) live_samples: Vec<LiveSample>,
    pub(crate) mapping_snapshots: Vec<MappingSnapshot>,
    pub(crate) sampling_error: Option<String>,
}
#[derive(Clone)]
pub(crate) struct SystemActivity {
    pub(crate) captured_at: u64,
    pub(crate) window_ms: u64,
    pub(crate) files: Vec<FileActivity>,
    pub(crate) ipc: Vec<IpcActivity>,
    pub(crate) cpu: Vec<CpuActivity>,
    pub(crate) signals: Vec<SignalEvent>,
    pub(crate) status: SystemMonitorStatus,
    pub(crate) raw_activity: Option<RawActivity>,
}
#[derive(Clone, Default)]
/// State and collection health of the system monitoring sensors.
pub(crate) struct SystemMonitorStatus {
    pub(crate) active: bool,
    pub(crate) ipc: SensorState,
    pub(crate) cpu: SensorState,
    pub(crate) files: SensorState,
    pub(crate) signals: SensorState,
    pub(crate) signals_lost: Option<u64>,
    pub(crate) coverage: Option<String>,
    pub(crate) files_coverage: Option<String>,
    pub(crate) lost: Option<u64>,
    pub(crate) unresolved: Option<u64>,
    pub(crate) files_lost: Option<u64>,
}
/// Health of an acquisition sensor; unavailable and error states carry a diagnostic.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) enum SensorState {
    #[default]
    Idle,
    Observing,
    Unavailable(String),
    Error(String),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SampleSource {
    CpuClock,
    Ptrace,
    ContextSwitch { preempted: bool },
}

#[derive(Clone, Debug)]
pub(crate) struct ObservationData {
    pub(crate) process_sample: ProcessSample,
    pub(crate) threads_only: bool,
}
#[derive(Clone, Debug)]
pub(crate) struct EnvironmentData {
    pub(crate) process_id: ProcessId,
    pub(crate) captured_at: u64,
    pub(crate) raw_environment: Vec<u8>,
}
#[derive(Clone, Debug)]
pub(crate) struct AuxData {
    pub(crate) process_id: ProcessId,
    pub(crate) captured_at: u64,
    pub(crate) word_bits: usize,
    pub(crate) raw_auxv: Vec<u8>,
    pub(crate) raw_aux_strings: Vec<(u64, Option<String>, Option<String>)>,
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
