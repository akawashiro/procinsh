//! Mapping values, classification and parsing of captured maps and smaps.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`deserialize_hex`] | `pub(crate)` | `fn deserialize_hex<'de, D: `[`serde::Deserializer`]`<'de>>(deserializer: D) -> `[`Result`]`<u64, D::Error>` |
//! | [`hex`] | `pub(crate)` | `fn hex<S: `[`serde::Serializer`]`>(value: &u64, serializer: S) -> `[`Result`]`<S::Ok, S::Error>` |

use crate::ipc::model::memory::MemoryMap;
use crate::{http_server::model::resource::decimal, ipc::model::resource::DeviceId};
use anyhow::Result;
use serde::Deserialize;

pub(crate) fn deserialize_hex<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<u64, D::Error> {
    let value = String::deserialize(deserializer)?;
    u64::from_str_radix(value.trim_start_matches("0x"), 16).map_err(serde::de::Error::custom)
}

#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::model::memory::MemoryKind")]
#[serde(rename_all = "snake_case")]
enum MemoryKindJson {
    Integer,
    Stack,
    Heap,
    SharedLibrary,
    Executable,
    File,
    Anonymous,
}
impl serde::Serialize for crate::ipc::model::memory::MemoryKind {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        MemoryKindJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::model::memory::MemoryKind {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        MemoryKindJson::deserialize(d)
    }
}

/// Structural mapping without RSS/PSS measurements.
/// Addresses and file offsets serialize as hexadecimal strings; inodes as decimal strings.
#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::model::memory::MemoryMap")]
struct MemoryMapJson {
    #[serde(serialize_with = "hex", deserialize_with = "deserialize_hex")]
    start: u64,
    #[serde(serialize_with = "hex", deserialize_with = "deserialize_hex")]
    end: u64,
    readable: bool,
    writable: bool,
    executable: bool,
    private: bool,
    #[serde(serialize_with = "hex", deserialize_with = "deserialize_hex")]
    file_offset: u64,
    device: DeviceId,
    #[serde(
        serialize_with = "decimal",
        deserialize_with = "crate::http_server::model::resource::deserialize_decimal"
    )]
    inode: u64,
    pathname: Option<String>,
}
impl serde::Serialize for crate::ipc::model::memory::MemoryMap {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        MemoryMapJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::model::memory::MemoryMap {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        MemoryMapJson::deserialize(d)
    }
}

/// A mapping with optional resident/proportional usage measured from smaps.
/// Serialization flattens the structural mapping alongside the usage measurements.
#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::model::memory::SmapsEntry")]
struct SmapsEntryJson {
    #[serde(flatten)]
    mapping: MemoryMap,
    rss_bytes: Option<u64>,
    pss_bytes: Option<u64>,
}
impl serde::Serialize for crate::ipc::model::memory::SmapsEntry {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        SmapsEntryJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::model::memory::SmapsEntry {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        SmapsEntryJson::deserialize(d)
    }
}

pub(crate) fn hex<S: serde::Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&format!("0x{value:016x}"))
}

/// One captured mapping context shared by all thread samples from that epoch.
#[derive(serde::Serialize, serde::Deserialize)]
#[allow(dead_code)]
#[serde(remote = "crate::ipc::model::memory::MappingSnapshot")]
struct MappingSnapshotJson {
    epoch: u64,
    maps: Vec<MemoryMap>,
}
impl serde::Serialize for crate::ipc::model::memory::MappingSnapshot {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        MappingSnapshotJson::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for crate::ipc::model::memory::MappingSnapshot {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        MappingSnapshotJson::deserialize(d)
    }
}

#[cfg(test)]
mod tests {
    use crate::ipc::model::memory::{SmapsEntry, parse_map, parse_smaps};

    #[test]
    fn maps_and_smaps_keep_paths_and_boundaries() {
        use crate::ipc::model::memory::memory_map_contains;

        let maps = parse_smaps("1000-2000 r-xp 00001000 08:01 10 /tmp/a b (deleted)\nRss: 4 kB\nPss: 2 kB\n2000-3000 rw-p 0 00:00 0\n").unwrap();
        assert_eq!(
            maps[0].mapping.pathname.as_deref(),
            Some("/tmp/a b (deleted)")
        );
        assert_eq!(maps[0].rss_bytes, Some(4096));
        let detailed = serde_json::to_value(&maps[0]).unwrap();
        assert_eq!(detailed["rss_bytes"], 4096);
        assert_eq!(detailed["pss_bytes"], 2048);
        assert_eq!(detailed["start"], "0x0000000000001000");
        assert!(detailed.get("mapping").is_none());
        assert!(memory_map_contains(&maps[0].mapping, 0x1000));
        assert!(!memory_map_contains(&maps[0].mapping, 0x2000));
        assert!(maps[1].mapping.pathname.is_none());
        assert!(parse_map("2000-1000 rw-p 0 00:00 0").is_err());
    }

    #[test]
    fn structural_maps_exclude_usage_and_fallback_observations_keep_shape() {
        let mapping = parse_map("1000-2000 r-xp 0 00:00 0 /tmp/test").unwrap();
        let structural = serde_json::to_value(&mapping).unwrap();
        assert!(structural.get("rss_bytes").is_none());
        assert!(structural.get("pss_bytes").is_none());
        let fallback = SmapsEntry::from(mapping.clone());
        let json = serde_json::to_value(&fallback).unwrap();
        assert!(json["rss_bytes"].is_null());
        assert!(json["pss_bytes"].is_null());
        assert_eq!(json["pathname"], "/tmp/test");
        let measured =
            parse_smaps("1000-2000 r-xp 0 00:00 0 /tmp/test\nRss: 8 kB\nPss: 4 kB\n").unwrap();
        assert_eq!(measured[0].mapping, mapping);
        assert_eq!(measured[0].rss_bytes, Some(8192));
    }
}
