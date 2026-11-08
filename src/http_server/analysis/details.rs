//! Format captured environment and auxiliary-vector values in main.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`EnvironmentEntry`] | `pub(crate)` | `struct EnvironmentEntry` |
//! | [`AuxEntry`] | `pub(crate)` | `struct AuxEntry` |
//! | [`format_process_metadata`] | `pub(in crate::http_server)` | `fn format_process_metadata(value: &mut `[`serde_json::Value`]`) -> `[`Result`]`<()>` |

use anyhow::{Result, bail, ensure};
use serde::Serialize;
#[derive(Debug, Serialize, serde::Deserialize)]
pub(crate) struct EnvironmentEntry {
    pub(crate) name: String,
    pub(crate) value: Option<String>,
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

#[derive(Debug, Serialize, serde::Deserialize)]
pub(crate) struct AuxEntry {
    tag: String,
    pub(crate) name: String,
    #[serde(
        serialize_with = "crate::http_server::model::memory::hex",
        deserialize_with = "crate::http_server::model::memory::deserialize_hex"
    )]
    pub(crate) value: u64,
    decimal: String,
    kind: String,
    description: String,
    pub(crate) text: Option<String>,
    text_error: Option<String>,
}

fn auxiliary_tag_metadata(tag: u64) -> (&'static str, &'static str, &'static str) {
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
        let (name, kind, description) = auxiliary_tag_metadata(tag);
        entries.push(AuxEntry {
            tag: tag.to_string(),
            name: if name == "AT_UNKNOWN" {
                format!("AT_UNKNOWN_{tag}")
            } else {
                name.into()
            },
            value,
            decimal: value.to_string(),
            kind: kind.into(),
            description: description.into(),
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

pub(in crate::http_server) fn format_process_metadata(value: &mut serde_json::Value) -> Result<()> {
    if value.get("raw_environment").is_some() {
        let bytes: Vec<u8> = serde_json::from_value(
            value
                .as_object_mut()
                .unwrap()
                .remove("raw_environment")
                .unwrap(),
        )?;
        value["entries"] = serde_json::to_value(parse_environment(&bytes))?;
        value["lossy_utf8"] = serde_json::json!(std::str::from_utf8(&bytes).is_err());
    }
    if value.get("raw_auxv").is_some() {
        let bytes: Vec<u8> =
            serde_json::from_value(value.as_object_mut().unwrap().remove("raw_auxv").unwrap())?;
        let word_bytes = value["word_bits"].as_u64().unwrap() as usize / 8;
        let texts: Vec<(u64, Option<String>, Option<String>)> = serde_json::from_value(
            value
                .as_object_mut()
                .unwrap()
                .remove("raw_aux_strings")
                .unwrap(),
        )?;
        let mut entries = parse_auxv(&bytes, word_bytes)?;
        for entry in &mut entries {
            if let Some((_, text, error)) = texts
                .iter()
                .find(|(tag, _, _)| entry.tag == tag.to_string())
            {
                entry.text = text.clone();
                entry.text_error = error.clone();
            }
        }
        value["entries"] = serde_json::to_value(entries)?;
    }
    Ok(())
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
    }
}
