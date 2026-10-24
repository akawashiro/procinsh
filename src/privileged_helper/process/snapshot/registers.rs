use super::arch::native;
use crate::privileged_helper::process::maps::{MemoryKind, MemoryMap};
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
/// Architecture-independent purpose of a register, separate from the mapping `kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum RegisterRole {
    Pc,
    Sp,
    Fp,
    /// AArch64 X30; x86-64 has no link register.
    #[cfg_attr(not(target_arch = "aarch64"), allow(dead_code))]
    Lr,
    Flags,
    General,
}
/// Classified register value; its mapping-relative offset serializes as hex or null.
#[derive(Clone, Debug, Serialize)]
pub(super) struct Register {
    pub(super) name: String,
    pub(super) role: RegisterRole,
    #[serde(serialize_with = "crate::privileged_helper::process::maps::hex")]
    pub(super) value: u64,
    pub(super) decimal: String,
    pub(super) kind: MemoryKind,
    pub(super) mapping: Option<RegisterMapping>,
    #[serde(serialize_with = "optional_hex")]
    pub(super) offset: Option<u64>,
}

pub(super) fn classify(name: &str, role: RegisterRole, value: u64, maps: &[MemoryMap]) -> Register {
    let map = maps.iter().find(|m| m.contains(value));
    Register {
        name: name.into(),
        role,
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

/// Values indexed by the native PERF_REG_* numbering (on x86-64, unused segment
/// registers are zero); see `arch::native`.
#[derive(Clone, Debug)]
pub(super) struct RegisterSet(pub(super) [u64; 24]);
pub(super) fn from_sample(r: &RegisterSet, maps: &[MemoryMap]) -> Vec<Register> {
    native::REGISTERS
        .iter()
        .map(|&(name, role, index)| classify(name, role, r.0[index], maps))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_mapping_keeps_fields_and_offset_or_integer() {
        let map = crate::privileged_helper::process::maps::parse_map(
            "1000-2000 rw-p 00000000 08:01 42 /tmp/a [b]",
        )
        .unwrap();
        let register = classify("RAX", RegisterRole::General, 0x1008, &[map]);
        assert_eq!(register.kind, MemoryKind::File);
        assert_eq!(register.offset, Some(8));
        let json = serde_json::to_value(register).unwrap();
        assert_eq!(
            json["mapping"],
            serde_json::json!({"pathname":"/tmp/a [b]","readable":true,"writable":true,"executable":false,"private":true})
        );
        assert_eq!(json["offset"], "0x8");
        assert_eq!(json["role"], "general");
        let integer = classify("RAX", RegisterRole::General, u64::MAX, &[]);
        assert_eq!(integer.kind, MemoryKind::Integer);
        assert!(integer.mapping.is_none() && integer.offset.is_none());
        assert_eq!(
            serde_json::to_value(integer).unwrap()["offset"],
            serde_json::Value::Null
        );
    }

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn x86_64_sample_keeps_order_and_marks_pc_sp_fp_and_flags() {
        let registers = from_sample(&RegisterSet([0; 24]), &[]);
        let roles: Vec<_> = registers
            .iter()
            .filter(|r| r.role != RegisterRole::General)
            .map(|r| (r.name.as_str(), r.role))
            .collect();
        assert_eq!(
            roles,
            [
                ("RIP", RegisterRole::Pc),
                ("RSP", RegisterRole::Sp),
                ("RBP", RegisterRole::Fp),
                ("RFLAGS", RegisterRole::Flags),
            ]
        );
        assert_eq!(registers.len(), 18);
        assert_eq!(registers[0].name, "RIP");
        assert_eq!(registers[17].name, "RFLAGS");
    }
}
