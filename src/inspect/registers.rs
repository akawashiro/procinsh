use crate::process::maps::MemoryMap;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Register {
    pub name: String,
    #[serde(serialize_with = "crate::process::maps::hex")]
    pub value: u64,
    pub decimal: String,
    pub kind: String,
    pub mapping: Option<String>,
    pub offset: Option<String>,
}

pub fn classify(name: &str, value: u64, maps: &[MemoryMap]) -> Register {
    let map = maps.iter().find(|m| m.contains(value));
    Register {
        name: name.into(),
        value,
        decimal: value.to_string(),
        kind: map.map_or("integer", |m| m.kind()).into(),
        mapping: map.map(|m| {
            format!(
                "{} [{}]",
                m.pathname.as_deref().unwrap_or("[anonymous]"),
                m.permissions
            )
        }),
        offset: map.map(|m| format!("0x{:x}", value - m.start)),
    }
}

/// Values arrive in ascending perf register-mask bit order, not ptrace order.
pub const MASK: u64 = 0xff03ff;
pub const NAMES: [(u32, &str); 18] = [
    (0, "RAX"),
    (1, "RBX"),
    (2, "RCX"),
    (3, "RDX"),
    (4, "RSI"),
    (5, "RDI"),
    (6, "RBP"),
    (7, "RSP"),
    (8, "RIP"),
    (9, "RFLAGS"),
    (16, "R8"),
    (17, "R9"),
    (18, "R10"),
    (19, "R11"),
    (20, "R12"),
    (21, "R13"),
    (22, "R14"),
    (23, "R15"),
];
pub fn from_perf(values: &[(u32, u64)], maps: &[MemoryMap]) -> Vec<Register> {
    values
        .iter()
        .filter_map(|(bit, value)| {
            NAMES
                .iter()
                .find(|(b, _)| b == bit)
                .map(|(_, name)| classify(name, *value, maps))
        })
        .collect()
}
