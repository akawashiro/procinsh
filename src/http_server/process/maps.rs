// Visibility is scoped to the consumers of the parent process façade.
use crate::http_server::resource::{DeviceId, decimal};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::fs;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::http_server) enum MemoryKind {
    Integer,
    Stack,
    Heap,
    SharedLibrary,
    Executable,
    File,
    Anonymous,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(in crate::http_server) struct MemoryMap {
    #[serde(serialize_with = "hex")]
    pub(in crate::http_server) start: u64,
    #[serde(serialize_with = "hex")]
    pub(in crate::http_server) end: u64,
    pub(in crate::http_server) readable: bool,
    pub(in crate::http_server) writable: bool,
    pub(in crate::http_server) executable: bool,
    pub(in crate::http_server) private: bool,
    #[serde(serialize_with = "hex")]
    pub(in crate::http_server) file_offset: u64,
    pub(in crate::http_server) device: DeviceId,
    #[serde(serialize_with = "decimal")]
    pub(in crate::http_server) inode: u64,
    pub(in crate::http_server) pathname: Option<String>,
}

/// A mapping with optional resident/proportional usage measured from smaps.
#[derive(Clone, Debug, Serialize)]
pub(super) struct MemoryMapObservation {
    #[serde(flatten)]
    pub(super) mapping: MemoryMap,
    pub(super) rss_bytes: Option<u64>,
    pub(super) pss_bytes: Option<u64>,
}
impl From<MemoryMap> for MemoryMapObservation {
    fn from(mapping: MemoryMap) -> Self {
        Self {
            mapping,
            rss_bytes: None,
            pss_bytes: None,
        }
    }
}

pub(super) fn hex<S: serde::Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&format!("0x{value:016x}"))
}
impl MemoryMap {
    pub(super) fn contains(&self, address: u64) -> bool {
        self.start <= address && address < self.end
    }

    pub(super) fn kind(&self) -> MemoryKind {
        match self.pathname.as_deref() {
            Some(p) if p.starts_with("[stack") => MemoryKind::Stack,
            Some("[heap]") => MemoryKind::Heap,
            Some(p) if p.contains(".so") => MemoryKind::SharedLibrary,
            _ if self.executable => MemoryKind::Executable,
            Some(p) if p.starts_with('/') => MemoryKind::File,
            _ => MemoryKind::Anonymous,
        }
    }
}

pub(super) fn parse_map(line: &str) -> Result<MemoryMap> {
    // Split only the first five columns: file names may contain spaces.
    let mut rest = line.trim();
    let mut columns = Vec::new();
    for _ in 0..5 {
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        ensure!(end > 0, "maps: missing field");
        columns.push(&rest[..end]);
        rest = rest[end..].trim_start();
    }
    let (start, end) = columns[0].split_once('-').context("maps: invalid range")?;
    let permissions = columns[1];
    ensure!(permissions.len() == 4, "maps: invalid permissions");
    let start = u64::from_str_radix(start, 16)?;
    let end = u64::from_str_radix(end, 16)?;
    ensure!(start < end, "maps: empty range");
    Ok(MemoryMap {
        start,
        end,
        readable: permissions.as_bytes()[0] == b'r',
        writable: permissions.as_bytes()[1] == b'w',
        executable: permissions.as_bytes()[2] == b'x',
        private: permissions.as_bytes()[3] == b'p',
        file_offset: u64::from_str_radix(columns[2], 16)?,
        device: {
            let (major, minor) = columns[3].split_once(':').context("maps: invalid device")?;
            DeviceId {
                major: u32::from_str_radix(major, 16)?,
                minor: u32::from_str_radix(minor, 16)?,
            }
        },
        inode: columns[4].parse()?,
        pathname: (!rest.is_empty()).then(|| rest.to_owned()),
    })
}

pub(super) fn parse_smaps(text: &str) -> Result<Vec<MemoryMapObservation>> {
    let mut maps: Vec<MemoryMapObservation> = Vec::new();
    for line in text.lines() {
        if line
            .split_whitespace()
            .next()
            .is_some_and(|word| word.contains('-'))
        {
            maps.push(parse_map(line)?.into());
        } else if let Some(map) = maps.last_mut()
            && let Some((key, value)) = line.split_once(':')
        {
            let bytes = value
                .split_whitespace()
                .next()
                .and_then(|v| v.parse::<u64>().ok())
                .and_then(|v| v.checked_mul(1024));
            match key {
                "Rss" => map.rss_bytes = bytes,
                "Pss" => map.pss_bytes = bytes,
                _ => {}
            }
        }
    }
    Ok(maps)
}

pub(super) fn read_maps(pid: i32) -> Result<Vec<MemoryMap>> {
    fs::read_to_string(format!("/proc/{pid}/maps"))?
        .lines()
        .map(parse_map)
        .collect()
}

pub(super) fn read_smaps(pid: i32) -> Result<Vec<MemoryMapObservation>> {
    if let Ok(text) = fs::read_to_string(format!("/proc/{pid}/smaps")) {
        return parse_smaps(&text);
    }
    Ok(read_maps(pid)?
        .into_iter()
        .map(MemoryMapObservation::from)
        .collect())
}

#[derive(Clone, Debug, Default, Serialize)]
pub(super) struct MemoryRollup {
    pub(super) rss_bytes: Option<u64>,
    pub(super) pss_bytes: Option<u64>,
    pub(super) private_bytes: Option<u64>,
}

pub(super) fn rollup(pid: i32) -> Option<MemoryRollup> {
    let f = super::procfs::fields(&format!("/proc/{pid}/smaps_rollup")).ok()?;
    let bytes = |k| super::procfs::field_u64(&f, k).and_then(|v| v.checked_mul(1024));
    Some(MemoryRollup {
        rss_bytes: bytes("Rss"),
        pss_bytes: bytes("Pss"),
        private_bytes: bytes("Private_Clean")
            .zip(bytes("Private_Dirty"))
            .map(|(a, b)| a + b),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_and_smaps_keep_paths_and_boundaries() {
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
        assert!(maps[0].mapping.contains(0x1000));
        assert!(!maps[0].mapping.contains(0x2000));
        assert!(maps[1].mapping.pathname.is_none());
        assert!(parse_map("2000-1000 rw-p 0 00:00 0").is_err());
    }

    #[test]
    fn structural_maps_exclude_usage_and_fallback_observations_keep_shape() {
        let mapping = parse_map("1000-2000 r-xp 0 00:00 0 /tmp/test").unwrap();
        let structural = serde_json::to_value(&mapping).unwrap();
        assert!(structural.get("rss_bytes").is_none());
        assert!(structural.get("pss_bytes").is_none());
        let fallback = MemoryMapObservation::from(mapping.clone());
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
