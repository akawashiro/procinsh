use super::{ProcessId, check_identity, memory, permission_help, timestamp_ms};
use anyhow::{Context, Result, bail, ensure};
use serde::Serialize;
use std::{fs::File, io::Read};

const MAX_ENVIRONMENT: usize = 1024 * 1024;
const MAX_AUXV: usize = 64 * 1024;

fn read_limited(reader: impl Read, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= limit,
        "The read limit of {limit} bytes was exceeded."
    );
    Ok(bytes)
}

#[derive(Debug, Serialize)]
pub struct EnvironmentEntry {
    pub name: String,
    pub value: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct Environment {
    pub process_id: ProcessId,
    pub captured_at: u64,
    pub entries: Vec<EnvironmentEntry>,
    pub lossy_utf8: bool,
}

fn parse_environment(bytes: &[u8]) -> Vec<EnvironmentEntry> {
    bytes
        .split(|b| *b == 0)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let (name, value) = match entry.iter().position(|b| *b == b'=') {
                Some(index) => (&entry[..index], Some(&entry[index + 1..])),
                None => (entry, None),
            };
            EnvironmentEntry {
                name: String::from_utf8_lossy(name).into_owned(),
                value: value.map(|v| String::from_utf8_lossy(v).into_owned()),
            }
        })
        .collect()
}

pub fn environment(id: ProcessId) -> Result<Environment> {
    check_identity(id)?;
    let result = (|| -> Result<Vec<u8>> {
        read_limited(
            File::open(format!("/proc/{}/environ", id.pid))?,
            MAX_ENVIRONMENT,
        )
    })();
    check_identity(id)?;
    let bytes = result.map_err(|e| anyhow::anyhow!(permission_help("/proc/PID/environ", e)))?;
    Ok(Environment {
        process_id: id,
        captured_at: timestamp_ms(),
        entries: parse_environment(&bytes),
        lossy_utf8: std::str::from_utf8(&bytes).is_err(),
    })
}

#[derive(Debug, Serialize)]
pub struct AuxEntry {
    pub tag: String,
    pub name: String,
    #[serde(serialize_with = "super::maps::hex")]
    pub value: u64,
    pub decimal: String,
    pub kind: &'static str,
    pub description: &'static str,
    pub text: Option<String>,
    pub text_error: Option<String>,
}
#[derive(Debug, Serialize)]
pub struct AuxVector {
    pub process_id: ProcessId,
    pub captured_at: u64,
    pub word_bits: u8,
    pub entries: Vec<AuxEntry>,
}

fn aux_info(tag: u64) -> (&'static str, &'static str, &'static str) {
    match tag {
        0 => ("AT_NULL", "number", "End of vector"),
        1 => ("AT_IGNORE", "number", "Ignored entry"),
        2 => ("AT_EXECFD", "number", "Executable file descriptor"),
        3 => ("AT_PHDR", "address", "Program header address"),
        4 => ("AT_PHENT", "number", "Size of one program header"),
        5 => ("AT_PHNUM", "number", "Number of program headers"),
        6 => ("AT_PAGESZ", "number", "Page size (bytes)"),
        7 => ("AT_BASE", "address", "ELF interpreter base address"),
        8 => ("AT_FLAGS", "bitmask", "Flags"),
        9 => ("AT_ENTRY", "address", "Program entry point"),
        10 => ("AT_NOTELF", "number", "Non-ELF program"),
        11 => ("AT_UID", "number", "Real UID"),
        12 => ("AT_EUID", "number", "Effective UID"),
        13 => ("AT_GID", "number", "Real GID"),
        14 => ("AT_EGID", "number", "Effective GID"),
        15 => ("AT_PLATFORM", "address", "Platform string"),
        16 => ("AT_HWCAP", "bitmask", "CPU feature bitmask (ABI-specific)"),
        17 => ("AT_CLKTCK", "number", "Clock ticks / sec"),
        23 => ("AT_SECURE", "number", "secure-execution mode"),
        24 => ("AT_BASE_PLATFORM", "address", "Base platform string"),
        25 => ("AT_RANDOM", "address", "Address of 16 random bytes"),
        26 => ("AT_HWCAP2", "bitmask", "CPU feature bitmask 2"),
        27 => ("AT_RSEQ_FEATURE_SIZE", "number", "rseq feature size"),
        28 => ("AT_RSEQ_ALIGN", "number", "rseq alignment"),
        29 => ("AT_HWCAP3", "bitmask", "CPU feature bitmask 3"),
        30 => ("AT_HWCAP4", "bitmask", "CPU feature bitmask 4"),
        31 => ("AT_EXECFN", "address", "Executable filename at startup"),
        32 => ("AT_SYSINFO", "address", "vDSO system call function"),
        33 => ("AT_SYSINFO_EHDR", "address", "vDSO ELF header"),
        51 => (
            "AT_MINSIGSTKSZ",
            "number",
            "Minimum stack for signal delivery (bytes)",
        ),
        _ => ("AT_UNKNOWN", "unknown", "Unknown tag (raw value)"),
    }
}

fn parse_auxv(bytes: &[u8], word_bytes: usize) -> Result<Vec<AuxEntry>> {
    ensure!(matches!(word_bytes, 4 | 8), "unsupported auxv word size");
    let mut entries = Vec::new();
    for pair in bytes.chunks_exact(word_bytes * 2) {
        let word = |b: &[u8]| {
            if word_bytes == 8 {
                u64::from_le_bytes(b.try_into().unwrap())
            } else {
                u32::from_le_bytes(b.try_into().unwrap()) as u64
            }
        };
        let tag = word(&pair[..word_bytes]);
        let value = word(&pair[word_bytes..]);
        let (name, kind, description) = aux_info(tag);
        entries.push(AuxEntry {
            tag: tag.to_string(),
            name: if name == "AT_UNKNOWN" {
                format!("AT_UNKNOWN_{tag}")
            } else {
                name.into()
            },
            value,
            decimal: value.to_string(),
            kind,
            description,
            text: None,
            text_error: None,
        });
        if tag == 0 {
            ensure!(value == 0, "auxv: invalid AT_NULL value");
            return Ok(entries);
        }
    }
    bail!("Incomplete auxv (AT_NULL is missing).")
}

fn elf_word_bytes(ident: &[u8]) -> Result<usize> {
    ensure!(
        ident.len() >= 16 && &ident[..4] == b"\x7fELF",
        "Could not identify the ELF header."
    );
    ensure!(ident[5] == 1, "Only little-endian ELF is supported.");
    match ident[4] {
        1 => Ok(4),
        2 => Ok(8),
        _ => bail!("unsupported ELF class"),
    }
}

fn read_string(pid: i32, address: u64) -> Result<String> {
    let bytes = memory::read_raw(pid, address, 4096)?;
    let end = bytes
        .iter()
        .position(|b| *b == 0)
        .context("No string terminator found within 4096 bytes.")?;
    Ok(String::from_utf8_lossy(&bytes[..end]).into_owned())
}

pub fn auxv(id: ProcessId) -> Result<AuxVector> {
    check_identity(id)?;
    let result = (|| -> Result<(usize, Vec<AuxEntry>)> {
        // Read the target's ELF class instead of assuming the inspector's word size.
        let mut ident = [0u8; 16];
        File::open(format!("/proc/{}/exe", id.pid))?.read_exact(&mut ident)?;
        let word_bytes = elf_word_bytes(&ident)?;
        let bytes = read_limited(File::open(format!("/proc/{}/auxv", id.pid))?, MAX_AUXV)?;
        let mut entries = parse_auxv(&bytes, word_bytes)?;
        for entry in &mut entries {
            if matches!(
                entry.name.as_str(),
                "AT_EXECFN" | "AT_PLATFORM" | "AT_BASE_PLATFORM"
            ) && entry.value != 0
            {
                match read_string(id.pid, entry.value) {
                    Ok(text) => entry.text = Some(text),
                    Err(error) => entry.text_error = Some(error.to_string()),
                }
            }
        }
        Ok((word_bytes, entries))
    })();
    check_identity(id)?;
    let (word_bytes, entries) =
        result.map_err(|e| anyhow::anyhow!(permission_help("/proc/PID/auxv", e)))?;
    Ok(AuxVector {
        process_id: id,
        captured_at: timestamp_ms(),
        word_bits: (word_bytes * 8) as u8,
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn environment_preserves_duplicates_empty_values_and_equals() {
        let entries = parse_environment(b"X=a=b\0EMPTY=\0X=second\0ODD\0RAW=\xff\0");
        assert_eq!(entries.len(), 5);
        assert_eq!(entries[0].value.as_deref(), Some("a=b"));
        assert_eq!(entries[1].value.as_deref(), Some(""));
        assert_eq!(entries[2].name, "X");
        assert!(entries[3].value.is_none());
        assert_eq!(entries[4].value.as_deref(), Some("\u{fffd}"));
        assert!(parse_environment(b"").is_empty());
        assert_eq!(parse_environment(b"X=last").len(), 1);
        assert!(read_limited(&b"1234"[..], 3).is_err());
    }

    #[test]
    fn auxv_handles_32_and_64_bits_unknown_tags_and_terminator() {
        for word_bytes in [4, 8] {
            let mut bytes = Vec::new();
            for word in [6u64, 4096, 9, 0xf1234567, 999, 42, 0, 0] {
                bytes.extend_from_slice(&word.to_le_bytes()[..word_bytes]);
            }
            let entries = parse_auxv(&bytes, word_bytes).unwrap();
            assert_eq!(entries[0].name, "AT_PAGESZ");
            assert_eq!(entries[1].value, 0xf1234567);
            assert_eq!(entries[1].kind, "address");
            assert_eq!(entries[2].name, "AT_UNKNOWN_999");
            assert_eq!(entries[3].name, "AT_NULL");
            assert!(parse_auxv(&bytes[..bytes.len() - 1], word_bytes).is_err());
            bytes.extend_from_slice(&[123]); // padding past AT_NULL is ignored
            assert_eq!(parse_auxv(&bytes, word_bytes).unwrap().len(), 4);
        }
        assert!(parse_auxv(&[], 8).is_err());
        let mut ident = *b"\x7fELF\x02\x01\x01\0\0\0\0\0\0\0\0\0";
        assert_eq!(elf_word_bytes(&ident).unwrap(), 8);
        ident[4] = 1;
        assert_eq!(elf_word_bytes(&ident).unwrap(), 4);
        ident[5] = 2;
        assert!(elf_word_bytes(&ident).is_err());
    }
}
