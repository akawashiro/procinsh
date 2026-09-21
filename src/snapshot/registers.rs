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

pub fn from_raw(r: &libc::user_regs_struct, maps: &[MemoryMap]) -> Vec<Register> {
    [
        ("RIP", r.rip),
        ("RSP", r.rsp),
        ("RBP", r.rbp),
        ("RAX", r.rax),
        ("RBX", r.rbx),
        ("RCX", r.rcx),
        ("RDX", r.rdx),
        ("RSI", r.rsi),
        ("RDI", r.rdi),
        ("R8", r.r8),
        ("R9", r.r9),
        ("R10", r.r10),
        ("R11", r.r11),
        ("R12", r.r12),
        ("R13", r.r13),
        ("R14", r.r14),
        ("R15", r.r15),
        ("RFLAGS", r.eflags),
    ]
    .into_iter()
    .map(|(name, value)| classify(name, value, maps))
    .collect()
}
