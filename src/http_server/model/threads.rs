//! Raw thread counters and interpreted thread values.
//!
//! # Interface
//!
//! No named types or functions are exported; shared values come from their definition modules.

use crate::ipc::model::threads::{CpuRange, SchedulerPolicy, ThreadObservation, ThreadSample};
#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::model::threads::SchedulerPolicy")]
#[serde(tag = "kind", content = "code", rename_all = "snake_case")]
enum SchedulerPolicyJson {
    Other,
    Fifo,
    Rr,
    Batch,
    Idle,
    Deadline,
    Ext,
    Unknown(u32),
}
impl serde::Serialize for crate::ipc::model::threads::SchedulerPolicy {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        SchedulerPolicyJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::model::threads::SchedulerPolicy {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        SchedulerPolicyJson::deserialize(d)
    }
}

/// Inclusive range of CPU indices in a thread's affinity mask.
#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::model::threads::CpuRange")]
struct CpuRangeJson {
    pub start: u32,
    pub end: u32,
}
impl serde::Serialize for crate::ipc::model::threads::CpuRange {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        CpuRangeJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::model::threads::CpuRange {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        CpuRangeJson::deserialize(d)
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::model::threads::ThreadObservation")]
struct ThreadObservationJson {
    pub tid: i32,
    pub name: String,
    pub state: String,
    pub cpu: i32,
    pub cpu_percent: Option<f64>,
    pub priority: i64,
    pub nice: i64,
    pub scheduler: SchedulerPolicy,
    pub affinity: Option<Vec<CpuRange>>,
    pub voluntary_context_switches: Option<u64>,
    pub nonvoluntary_context_switches: Option<u64>,
}
impl serde::Serialize for crate::ipc::model::threads::ThreadObservation {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        ThreadObservationJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::model::threads::ThreadObservation {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        ThreadObservationJson::deserialize(d)
    }
}

/// Raw thread metadata and counters; CPU percentages are derived between observations.
#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::model::threads::ThreadSample")]
struct ThreadSampleJson {
    pub tid: i32,
    pub name: String,
    pub state: String,
    pub cpu: i32,
    pub priority: i64,
    pub nice: i64,
    pub scheduler: SchedulerPolicy,
    pub affinity: Option<Vec<CpuRange>>,
    pub voluntary_context_switches: Option<u64>,
    pub nonvoluntary_context_switches: Option<u64>,
    pub ticks: u64,
    pub start_time: u64,
}
impl serde::Serialize for crate::ipc::model::threads::ThreadSample {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        ThreadSampleJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::model::threads::ThreadSample {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        ThreadSampleJson::deserialize(d)
    }
}

impl ThreadSample {
    pub fn observation(&self) -> ThreadObservation {
        ThreadObservation {
            tid: self.tid,
            name: self.name.clone(),
            state: self.state.clone(),
            cpu: self.cpu,
            priority: self.priority,
            nice: self.nice,
            scheduler: self.scheduler,
            affinity: self.affinity.clone(),
            voluntary_context_switches: self.voluntary_context_switches,
            nonvoluntary_context_switches: self.nonvoluntary_context_switches,
            cpu_percent: None,
        }
    }
}

#[cfg(test)]
mod tests {

    use crate::ipc::model::threads::{CpuRange, SchedulerPolicy, parse_cpu_affinity};

    #[test]
    fn affinity_ranges_and_unknown_scheduler_keep_values() {
        assert_eq!(
            parse_cpu_affinity("0-3,8"),
            Some(vec![
                CpuRange { start: 0, end: 3 },
                CpuRange { start: 8, end: 8 }
            ])
        );
        assert_eq!(
            parse_cpu_affinity("4294967295"),
            Some(vec![CpuRange {
                start: u32::MAX,
                end: u32::MAX
            }])
        );
        for text in ["", "3-1", "1,", "1-2-3", "4294967296", "x"] {
            assert!(parse_cpu_affinity(text).is_none());
        }
        assert_eq!(
            serde_json::to_value(SchedulerPolicy::from_code(99)).unwrap(),
            serde_json::json!({"kind":"unknown","code":99})
        );
    }
}
