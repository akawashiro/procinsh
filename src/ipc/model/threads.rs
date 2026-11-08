//! Raw thread counters and interpreted thread values.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`SchedulerPolicy`] | `pub` | `enum SchedulerPolicy` |
//! | [`CpuRange`] | `pub` | `struct CpuRange` |
//! | [`ThreadObservation`] | `pub` | `struct ThreadObservation` |
//! | [`ThreadSample`] | `pub` | `struct ThreadSample` |
//! | [`scheduler_policy_from_code`] | `pub` | `fn scheduler_policy_from_code(code: u32) -> `[`SchedulerPolicy`] |
//! | [`parse_cpu_affinity`] | `pub` | `fn parse_cpu_affinity(text: &str) -> `[`Option`]`<`[`Vec`]`<`[`CpuRange`]`>>` |

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchedulerPolicy {
    Other,
    Fifo,
    Rr,
    Batch,
    Idle,
    Deadline,
    Ext,
    Unknown(u32),
}
pub fn scheduler_policy_from_code(code: u32) -> SchedulerPolicy {
    match code {
        0 => SchedulerPolicy::Other,
        1 => SchedulerPolicy::Fifo,
        2 => SchedulerPolicy::Rr,
        3 => SchedulerPolicy::Batch,
        5 => SchedulerPolicy::Idle,
        6 => SchedulerPolicy::Deadline,
        7 => SchedulerPolicy::Ext,
        n => SchedulerPolicy::Unknown(n),
    }
}
/// Inclusive range of CPU indices in a thread's affinity mask.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuRange {
    pub start: u32,
    pub end: u32,
}

pub fn parse_cpu_affinity(text: &str) -> Option<Vec<CpuRange>> {
    text.split(',')
        .map(|item| {
            let (start, end) = item.split_once('-').unwrap_or((item, item));
            let start = start.parse().ok()?;
            let end = end.parse().ok()?;
            (start <= end).then_some(CpuRange { start, end })
        })
        .collect()
}

#[derive(Clone, Debug)]
pub struct ThreadObservation {
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

/// Raw thread metadata and counters; CPU percentages are derived between observations.
#[derive(Clone, Debug)]
pub struct ThreadSample {
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

crate::ipc::codec::wire_enum!(SchedulerPolicy{0=>Other,1=>Fifo,2=>Rr,3=>Batch,4=>Idle,5=>Deadline,6=>Ext,7=>Unknown(value)});

crate::ipc::codec::wire_struct!(CpuRange { start, end });

crate::ipc::codec::wire_struct!(ThreadObservation {
    tid,
    name,
    state,
    cpu,
    cpu_percent,
    priority,
    nice,
    scheduler,
    affinity,
    voluntary_context_switches,
    nonvoluntary_context_switches
});

crate::ipc::codec::wire_struct!(ThreadSample {
    tid,
    name,
    state,
    cpu,
    priority,
    nice,
    scheduler,
    affinity,
    voluntary_context_switches,
    nonvoluntary_context_switches,
    ticks,
    start_time
});
