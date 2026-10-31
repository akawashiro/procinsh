//! Acquire bounded environment bytes, auxv words and selected target strings.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`environment`] | `pub(super)` | `fn environment(id: ProcessId) -> Result<crate::ipc::acquisition::EnvironmentData>` |
//! | [`auxv`] | `pub(super)` | `fn auxv(id: ProcessId) -> Result<crate::ipc::acquisition::AuxData>` |

use super::{ProcessId, memory, permission_help, timestamp_ms, validate_process_identity};
use anyhow::{Context, Result, bail, ensure};
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

pub(super) fn environment(id: ProcessId) -> Result<crate::ipc::acquisition::EnvironmentData> {
    ensure!(
        !memory::privacy(),
        "Environment variables are not read in privacy mode."
    );
    validate_process_identity(id)?;
    let bytes = read_limited(
        File::open(format!("/proc/{}/environ", id.pid))?,
        MAX_ENVIRONMENT,
    )
    .map_err(|e| anyhow::anyhow!(permission_help("/proc/PID/environ", e)))?;
    validate_process_identity(id)?;
    Ok(crate::ipc::acquisition::EnvironmentData {
        process_id: id,
        captured_at: timestamp_ms(),
        raw_environment: bytes,
    })
}

pub(super) fn auxv(id: ProcessId) -> Result<crate::ipc::acquisition::AuxData> {
    validate_process_identity(id)?;
    let mut ident = [0; 16];
    File::open(format!("/proc/{}/exe", id.pid))?.read_exact(&mut ident)?;
    let width = elf_word_bytes(&ident)?;
    let bytes = read_limited(File::open(format!("/proc/{}/auxv", id.pid))?, MAX_AUXV)
        .map_err(|e| anyhow::anyhow!(permission_help("/proc/PID/auxv", e)))?;
    let mut strings = Vec::new();
    let mut terminated = false;
    for pair in bytes.chunks_exact(width * 2) {
        let word = |part: &[u8]| {
            if width == 8 {
                u64::from_le_bytes(part.try_into().unwrap())
            } else {
                u32::from_le_bytes(part.try_into().unwrap()) as u64
            }
        };
        let tag = word(&pair[..width]);
        let address = word(&pair[width..]);
        if tag == 0 {
            ensure!(address == 0, "auxv: invalid AT_NULL value");
            terminated = true;
            break;
        }
        if matches!(tag, 15 | 24 | 31) && address != 0 {
            let (text, error) = match read_string(id.pid, address) {
                Ok(text) => (Some(text), None),
                Err(error) => (None, Some(error.to_string())),
            };
            strings.push((tag, text, error));
        }
    }
    ensure!(terminated, "Incomplete auxv (AT_NULL is missing).");
    validate_process_identity(id)?;
    Ok(crate::ipc::acquisition::AuxData {
        process_id: id,
        captured_at: timestamp_ms(),
        word_bits: width * 8,
        raw_auxv: bytes,
        raw_aux_strings: strings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http_server::analysis::details::{parse_auxv, parse_environment};

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
