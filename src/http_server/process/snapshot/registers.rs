use crate::http_server::process::maps::MemoryMap;
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub(super) struct Register {
    pub(super) name: String,
    #[serde(serialize_with = "crate::http_server::process::maps::hex")]
    pub(super) value: u64,
    pub(super) decimal: String,
    pub(super) kind: String,
    pub(super) mapping: Option<String>,
    pub(super) offset: Option<String>,
}

pub(super) fn classify(name: &str, value: u64, maps: &[MemoryMap]) -> Register {
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

pub(super) fn from_raw(r: &libc::user_regs_struct, maps: &[MemoryMap]) -> Vec<Register> {
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
