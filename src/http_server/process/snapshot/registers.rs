use crate::http_server::process::maps::{MemoryKind, MemoryMap};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub(super) struct RegisterMapping {
    pub(super) pathname: Option<String>,
    pub(super) readable: bool,
    pub(super) writable: bool,
    pub(super) executable: bool,
    pub(super) private: bool,
}

fn optional_hex<S: serde::Serializer>(
    value: &Option<u64>,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    match value {
        Some(value) => serializer.serialize_some(&format!("0x{value:x}")),
        None => serializer.serialize_none(),
    }
}
#[derive(Clone, Debug, Serialize)]
pub(super) struct Register {
    pub(super) name: String,
    #[serde(serialize_with = "crate::http_server::process::maps::hex")]
    pub(super) value: u64,
    pub(super) decimal: String,
    pub(super) kind: MemoryKind,
    pub(super) mapping: Option<RegisterMapping>,
    #[serde(serialize_with = "optional_hex")]
    pub(super) offset: Option<u64>,
}

pub(super) fn classify(name: &str, value: u64, maps: &[MemoryMap]) -> Register {
    let map = maps.iter().find(|m| m.contains(value));
    Register {
        name: name.into(),
        value,
        decimal: value.to_string(),
        kind: map.map_or(MemoryKind::Integer, MemoryMap::kind),
        mapping: map.map(|m| RegisterMapping {
            pathname: m.pathname.clone(),
            readable: m.readable,
            writable: m.writable,
            executable: m.executable,
            private: m.private,
        }),
        offset: map.map(|m| value - m.start),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_mapping_keeps_fields_and_offset_or_integer() {
        let map = crate::http_server::process::maps::parse_map(
            "1000-2000 rw-p 00000000 08:01 42 /tmp/a [b]",
        )
        .unwrap();
        let register = classify("RAX", 0x1008, &[map]);
        assert_eq!(register.kind, MemoryKind::File);
        assert_eq!(register.offset, Some(8));
        let json = serde_json::to_value(register).unwrap();
        assert_eq!(
            json["mapping"],
            serde_json::json!({"pathname":"/tmp/a [b]","readable":true,"writable":true,"executable":false,"private":true})
        );
        assert_eq!(json["offset"], "0x8");
        let integer = classify("RAX", u64::MAX, &[]);
        assert_eq!(integer.kind, MemoryKind::Integer);
        assert!(integer.mapping.is_none() && integer.offset.is_none());
        assert_eq!(
            serde_json::to_value(integer).unwrap()["offset"],
            serde_json::Value::Null
        );
    }
}
