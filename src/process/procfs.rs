use anyhow::{Context, Result, anyhow, ensure};
use serde::Serialize;
use std::{collections::HashMap, fs};

#[derive(Debug, Clone)]
pub struct Stat {
    pub pid: i32,
    pub parent_pid: i32,
    pub name: String,
    pub state: String,
    pub ticks: u64,
    pub start_time: u64,
    pub minor_faults: u64,
    pub major_faults: u64,
    pub priority: i64,
    pub nice: i64,
    pub thread_count: u32,
    pub vms: u64,
    pub rss: u64,
    pub cpu: i32,
    pub policy: u32,
}

pub fn page_size() -> u64 {
    unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(1) as u64
}
pub fn ticks_per_second() -> f64 {
    unsafe { libc::sysconf(libc::_SC_CLK_TCK) }.max(1) as f64
}

pub fn parse_stat(text: &str) -> Result<Stat> {
    let open = text.find('(').context("stat: missing comm")?;
    let close = text.rfind(')').context("stat: missing comm end")?;
    ensure!(close > open, "stat: invalid comm");
    let f: Vec<_> = text[close + 1..].split_whitespace().collect();
    let number = |i: usize| -> Result<u64> {
        f.get(i)
            .context("stat: missing field")?
            .parse()
            .context("stat: invalid number")
    };
    let signed = |i: usize| -> Result<i64> {
        f.get(i)
            .context("stat: missing field")?
            .parse()
            .context("stat: invalid signed number")
    };
    Ok(Stat {
        pid: text[..open].trim().parse()?,
        parent_pid: signed(1)? as i32,
        name: text[open + 1..close].to_owned(),
        state: f.first().context("stat: missing state")?.to_string(),
        ticks: number(11)?.saturating_add(number(12)?),
        start_time: number(19)?,
        minor_faults: number(7)?,
        major_faults: number(9)?,
        priority: signed(15)?,
        nice: signed(16)?,
        thread_count: number(17)? as u32,
        vms: number(20)?,
        rss: (signed(21)?.max(0) as u64).saturating_mul(page_size()),
        cpu: signed(36)? as i32,
        policy: number(38)? as u32,
    })
}

pub fn read_stat(path: &str) -> Result<Stat> {
    parse_stat(&fs::read_to_string(path).with_context(|| format!("read {path}"))?)
}
pub fn fields(path: &str) -> Result<HashMap<String, String>> {
    Ok(fs::read_to_string(path)?
        .lines()
        .filter_map(|line| line.split_once(':'))
        .map(|(k, v)| (k.to_owned(), v.trim().to_owned()))
        .collect())
}
pub fn field_u64(fields: &HashMap<String, String>, key: &str) -> Option<u64> {
    fields.get(key)?.split_whitespace().next()?.parse().ok()
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct IoStats {
    pub read_bytes: u64,
    pub write_bytes: u64,
}
pub fn read_io(pid: i32) -> Result<IoStats> {
    let f = fields(&format!("/proc/{pid}/io"))?;
    Ok(IoStats {
        read_bytes: field_u64(&f, "read_bytes").ok_or_else(|| anyhow!("missing read_bytes"))?,
        write_bytes: field_u64(&f, "write_bytes").ok_or_else(|| anyhow!("missing write_bytes"))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stat_handles_spaces_and_parentheses_and_signed_priority() {
        let mut f = vec!["0"; 50];
        f[0] = "S";
        f[1] = "7";
        f[11] = "20";
        f[12] = "3";
        f[15] = "-40";
        f[19] = "918";
        f[21] = "-1";
        let stat = parse_stat(&format!("42 (a ) tricky (name)) {}", f.join(" "))).unwrap();
        assert_eq!(stat.name, "a ) tricky (name)");
        assert_eq!(stat.parent_pid, 7);
        assert_eq!(stat.ticks, 23);
        assert_eq!(stat.priority, -40);
        assert_eq!(stat.start_time, 918);
        assert_eq!(stat.rss, 0);
        assert!(parse_stat("42 (bad) S").is_err());
    }
}
