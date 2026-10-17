//! Replays captured Linux memory-map lines through four parser implementations.
//!
//! # Interface
//!
//! This binary exports no Rust types or functions. Its CLI is
//! `maps-parser-bench <maps-file> [iterations]`; it verifies equal parsed values
//! before timing rotating rounds and emitting JSON lines.

use anyhow::{Context, Result, ensure};
use std::{hint::black_box, time::Instant};
#[derive(Clone, Debug, PartialEq, Eq)]
struct DeviceId {
    major: u32,
    minor: u32,
}
#[derive(Clone, Debug, PartialEq, Eq)]
struct MemoryMap {
    start: u64,
    end: u64,
    readable: bool,
    writable: bool,
    executable: bool,
    private: bool,
    file_offset: u64,
    device: DeviceId,
    inode: u64,
    pathname: Option<String>,
}

fn unicode_vec(line: &str) -> Result<MemoryMap> {
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

fn unicode_array(line: &str) -> Result<MemoryMap> {
    // Split only the first five columns: file names may contain spaces.
    let mut rest = line.trim();
    let mut columns = [""; 5];
    for column in &mut columns {
        let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
        ensure!(end > 0, "maps: missing field");
        *column = &rest[..end];
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

fn ascii_array(line: &str) -> Result<MemoryMap> {
    // Split only the first five columns: file names may contain spaces.
    let mut rest = line.trim();
    let mut columns = [""; 5];
    for column in &mut columns {
        let end = rest
            .as_bytes()
            .iter()
            .position(|b| b.is_ascii_whitespace())
            .unwrap_or(rest.len());
        ensure!(end > 0, "maps: missing field");
        *column = &rest[..end];
        rest = rest[end..].trim_ascii_start();
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

fn simd_memchr3(line: &str) -> Result<MemoryMap> {
    // Split only the first five columns: file names may contain spaces.
    let mut rest = line.trim();
    let mut columns = [""; 5];
    for column in &mut columns {
        let end = memchr::memchr3(b' ', b'\t', b'\r', rest.as_bytes()).unwrap_or(rest.len());
        ensure!(end > 0, "maps: missing field");
        *column = &rest[..end];
        rest = rest[end..].trim_ascii_start();
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

fn cpu_time() -> f64 {
    let mut t = std::mem::MaybeUninit::<libc::timespec>::uninit();
    assert_eq!(
        unsafe { libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, t.as_mut_ptr()) },
        0
    );
    let t = unsafe { t.assume_init() };
    t.tv_sec as f64 + t.tv_nsec as f64 / 1e9
}
fn main() {
    let args: Vec<_> = std::env::args().collect();
    let text = std::fs::read_to_string(&args[1]).unwrap();
    let iterations: usize = args.get(2).map(|s| s.parse().unwrap()).unwrap_or(20);
    let variants: [(&str, fn(&str) -> Result<MemoryMap>); 4] = [
        ("unicode_vec", unicode_vec),
        ("unicode_array", unicode_array),
        ("ascii_array", ascii_array),
        ("simd_memchr3", simd_memchr3),
    ];
    let lines = text.lines().count();
    for line in text.lines() {
        let expected = unicode_vec(line).unwrap();
        for (_, f) in variants {
            assert_eq!(expected, f(line).unwrap());
        }
    }
    println!(
        "{{\"avx2_available\":{},\"lines\":{},\"iterations\":{}}}",
        std::is_x86_feature_detected!("avx2"),
        lines,
        iterations
    );
    for round in 0..5 {
        for shift in 0..4 {
            let (name, f) = variants[(shift + round) % 4];
            let wall = Instant::now();
            let cpu = cpu_time();
            for _ in 0..iterations {
                let parsed: Vec<_> = text
                    .lines()
                    .map(|line| f(black_box(line)).unwrap())
                    .collect();
                black_box(parsed);
            }
            let cpu = cpu_time() - cpu;
            println!(
                "{{\"round\":{},\"variant\":\"{}\",\"cpu_seconds\":{},\"wall_seconds\":{},\"million_rows_per_cpu_second\":{}}}",
                round,
                name,
                cpu,
                wall.elapsed().as_secs_f64(),
                lines as f64 * iterations as f64 / cpu / 1e6
            );
        }
    }
}
