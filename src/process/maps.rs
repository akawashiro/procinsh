use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::fs;

#[derive(Clone, Debug, Serialize)]
pub struct MemoryMap {
    #[serde(serialize_with = "hex")]
    pub start: u64,
    #[serde(serialize_with = "hex")]
    pub end: u64,
    pub readable: bool,
    pub writable: bool,
    pub executable: bool,
    pub private: bool,
    pub permissions: String,
    #[serde(serialize_with = "hex")]
    pub file_offset: u64,
    pub device: String,
    pub inode: u64,
    pub pathname: Option<String>,
    pub rss_bytes: Option<u64>,
    pub pss_bytes: Option<u64>,
}
pub fn hex<S: serde::Serializer>(value: &u64, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&format!("0x{value:016x}"))
}
impl MemoryMap {
    pub fn contains(&self, address: u64) -> bool {
        self.start <= address && address < self.end
    }
    pub fn kind(&self) -> &'static str {
        match self.pathname.as_deref() {
            Some(p) if p.starts_with("[stack") => "stack",
            Some("[heap]") => "heap",
            Some(p) if p.contains(".so") => "shared library",
            _ if self.executable => "executable",
            Some(p) if p.starts_with('/') => "file",
            _ => "anonymous",
        }
    }
}

pub fn parse_map(line: &str) -> Result<MemoryMap> {
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
        permissions: permissions.into(),
        file_offset: u64::from_str_radix(columns[2], 16)?,
        device: columns[3].into(),
        inode: columns[4].parse()?,
        pathname: (!rest.is_empty()).then(|| rest.to_owned()),
        rss_bytes: None,
        pss_bytes: None,
    })
}

pub fn parse_smaps(text: &str) -> Result<Vec<MemoryMap>> {
    let mut maps: Vec<MemoryMap> = Vec::new();
    for line in text.lines() {
        if line
            .split_whitespace()
            .next()
            .is_some_and(|word| word.contains('-'))
        {
            maps.push(parse_map(line)?);
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

pub fn read(pid: i32, detailed: bool) -> Result<Vec<MemoryMap>> {
    if detailed && let Ok(text) = fs::read_to_string(format!("/proc/{pid}/smaps")) {
        return parse_smaps(&text);
    }
    fs::read_to_string(format!("/proc/{pid}/maps"))?
        .lines()
        .map(parse_map)
        .collect()
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct MemoryRollup {
    pub rss_bytes: Option<u64>,
    pub pss_bytes: Option<u64>,
    pub private_bytes: Option<u64>,
}
pub fn rollup(pid: i32) -> Option<MemoryRollup> {
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
        assert_eq!(maps[0].pathname.as_deref(), Some("/tmp/a b (deleted)"));
        assert_eq!(maps[0].rss_bytes, Some(4096));
        assert!(maps[0].contains(0x1000));
        assert!(!maps[0].contains(0x2000));
        assert!(maps[1].pathname.is_none());
        assert!(parse_map("2000-1000 rw-p 0 00:00 0").is_err());
    }
}
