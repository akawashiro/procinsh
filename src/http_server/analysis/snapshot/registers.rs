//! Registers.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`RegisterMapping`] | `pub` | `struct RegisterMapping` |
//! | [`Register`] | `pub` | `struct Register` |
//! | [`classify`] | `pub` | `fn classify(name: &str, role: RegisterRole, value: u64, maps: &[MemoryMap]) -> Register` |
//! | [`RegisterRole`] | `pub` | `enum RegisterRole` |
//! | [`RegisterSet`] | `pub` | `struct RegisterSet` |
//! | [`from_sample`] | `pub` | `fn from_sample(r: &RegisterSet, maps: &[MemoryMap]) -> Vec<Register>` |
//!
//! Mapping arguments use [`MemoryMap`]; classification values use [`MemoryKind`].

use crate::http_server::model::memory::{MemoryKind, MemoryMap};
pub use crate::ipc::registers::{Register, RegisterMapping, RegisterRole, RegisterSet};

#[derive(serde::Serialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::registers::RegisterMapping")]
struct RegisterMappingJson {
    pub pathname: Option<String>,
    pub readable: bool,
    pub writable: bool,
    pub executable: bool,
    pub private: bool,
}
impl serde::Serialize for crate::ipc::registers::RegisterMapping {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        RegisterMappingJson::serialize(self, s)
    }
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
/// Classified register value; its mapping-relative offset serializes as hex or null.
#[derive(serde::Serialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::registers::Register")]
struct RegisterJson {
    pub name: String,
    pub role: RegisterRole,
    #[serde(serialize_with = "crate::http_server::model::memory::hex")]
    pub value: u64,
    pub decimal: String,
    pub kind: MemoryKind,
    pub mapping: Option<RegisterMapping>,
    #[serde(serialize_with = "optional_hex")]
    pub offset: Option<u64>,
}
impl serde::Serialize for crate::ipc::registers::Register {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        RegisterJson::serialize(self, s)
    }
}

pub fn classify(name: &str, role: RegisterRole, value: u64, maps: &[MemoryMap]) -> Register {
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

/// Private JSON values use the native Linux PERF_REG layout, with optional flags and PAC metadata.
#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::registers::RegisterSet")]
struct RegisterSetJson {
    values: Vec<u64>,
    flags_available: bool,
    code_pac_mask: Option<u64>,
}
impl serde::Serialize for RegisterSet {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.validate().map_err(serde::ser::Error::custom)?;
        RegisterSetJson::serialize(self, s)
    }
}
impl<'de> serde::Deserialize<'de> for RegisterSet {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let result = RegisterSetJson::deserialize(d)?;
        result.validate().map_err(serde::de::Error::custom)?;
        Ok(result)
    }
}
pub fn from_sample(r: &RegisterSet, maps: &[MemoryMap]) -> Vec<Register> {
    REGISTERS
        .iter()
        .filter(|&&(_, role, _)| role != RegisterRole::Flags || r.flags_available)
        .map(|&(name, role, index)| classify(name, role, r.values[index], maps))
        .collect()
}

#[cfg(target_arch = "x86_64")]
const REGISTERS: &[(&str, RegisterRole, usize)] = {
    use RegisterRole::{Flags, Fp, General, Pc, Sp};
    &[
        ("RIP", Pc, 8),
        ("RSP", Sp, 7),
        ("RBP", Fp, 6),
        ("RAX", General, 0),
        ("RBX", General, 1),
        ("RCX", General, 2),
        ("RDX", General, 3),
        ("RSI", General, 4),
        ("RDI", General, 5),
        ("R8", General, 16),
        ("R9", General, 17),
        ("R10", General, 18),
        ("R11", General, 19),
        ("R12", General, 20),
        ("R13", General, 21),
        ("R14", General, 22),
        ("R15", General, 23),
        ("RFLAGS", Flags, 9),
    ]
};

#[cfg(target_arch = "aarch64")]
const REGISTERS: &[(&str, RegisterRole, usize)] = &[
    ("PC", RegisterRole::Pc, 32),
    ("SP", RegisterRole::Sp, 31),
    ("X29", RegisterRole::Fp, 29),
    ("X30", RegisterRole::Lr, 30),
    ("PSTATE", RegisterRole::Flags, 33),
    ("X0", RegisterRole::General, 0),
    ("X1", RegisterRole::General, 1),
    ("X2", RegisterRole::General, 2),
    ("X3", RegisterRole::General, 3),
    ("X4", RegisterRole::General, 4),
    ("X5", RegisterRole::General, 5),
    ("X6", RegisterRole::General, 6),
    ("X7", RegisterRole::General, 7),
    ("X8", RegisterRole::General, 8),
    ("X9", RegisterRole::General, 9),
    ("X10", RegisterRole::General, 10),
    ("X11", RegisterRole::General, 11),
    ("X12", RegisterRole::General, 12),
    ("X13", RegisterRole::General, 13),
    ("X14", RegisterRole::General, 14),
    ("X15", RegisterRole::General, 15),
    ("X16", RegisterRole::General, 16),
    ("X17", RegisterRole::General, 17),
    ("X18", RegisterRole::General, 18),
    ("X19", RegisterRole::General, 19),
    ("X20", RegisterRole::General, 20),
    ("X21", RegisterRole::General, 21),
    ("X22", RegisterRole::General, 22),
    ("X23", RegisterRole::General, 23),
    ("X24", RegisterRole::General, 24),
    ("X25", RegisterRole::General, 25),
    ("X26", RegisterRole::General, 26),
    ("X27", RegisterRole::General, 27),
    ("X28", RegisterRole::General, 28),
];

impl serde::Serialize for RegisterRole {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(match self {
            Self::Pc => "pc",
            Self::Sp => "sp",
            Self::Fp => "fp",
            Self::Lr => "lr",
            Self::Flags => "flags",
            Self::General => "general",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_arch = "x86_64")]
    #[test]
    fn x86_64_sample_keeps_order_and_marks_pc_sp_fp_and_flags() {
        let registers = from_sample(
            &RegisterSet {
                flags_available: true,
                ..RegisterSet::default()
            },
            &[],
        );
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

    #[test]
    fn register_mapping_keeps_fields_and_offset_or_integer() {
        let map = crate::http_server::model::memory::parse_map(
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
    #[test]
    fn native_registers_round_trip_and_reject_wrong_counts() {
        let mut registers = RegisterSet {
            flags_available: true,
            code_pac_mask: Some(0xff00_0000_0000_0000),
            ..RegisterSet::default()
        };
        registers.values = (0..RegisterSet::COUNT).map(|i| i as u64 + 100).collect();
        let encoded = crate::ipc::codec::encode(&registers, 4096).unwrap();
        let decoded: RegisterSet = crate::ipc::codec::decode(&encoded).unwrap();
        assert_eq!(decoded.values, registers.values);
        assert_eq!(decoded.code_pac_mask, registers.code_pac_mask);
        assert!(decoded.flags_available);
        let json = serde_json::to_value(&registers).unwrap();
        let decoded: RegisterSet = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(decoded.pc(), 100 + RegisterSet::PC as u64);
        let mut invalid = json;
        invalid["values"].as_array_mut().unwrap().pop();
        assert!(serde_json::from_value::<RegisterSet>(invalid).is_err());
        let mut invalid = encoded;
        invalid[..4].copy_from_slice(&((RegisterSet::COUNT - 1) as u32).to_ne_bytes());
        assert!(crate::ipc::codec::decode::<RegisterSet>(&invalid).is_err());
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn arm64_names_roles_and_optional_pstate() {
        let mut r = RegisterSet {
            flags_available: true,
            ..RegisterSet::default()
        };
        r.values = (0..34).collect();
        let displayed = from_sample(&r, &[]);
        assert_eq!(displayed.len(), 34);
        for (name, role, value) in [
            ("PC", RegisterRole::Pc, 32),
            ("SP", RegisterRole::Sp, 31),
            ("X29", RegisterRole::Fp, 29),
            ("X30", RegisterRole::Lr, 30),
            ("PSTATE", RegisterRole::Flags, 33),
        ] {
            let register = displayed.iter().find(|r| r.name == name).unwrap();
            assert_eq!((register.role, register.value), (role, value));
        }
        r.flags_available = false;
        let displayed = from_sample(&r, &[]);
        assert_eq!(displayed.len(), 33);
        assert!(displayed.iter().all(|r| r.name != "PSTATE"));
    }
}
