//! Acquire bounded auxiliary-vector words and selected strings from target memory.
//!
//! # Interface
//! - [`read_auxiliary_vector`] (`pub(super)`): `fn read_auxiliary_vector(id: ProcessId) -> anyhow::Result<AuxData>`.
//!
//! Process identities use [`ProcessId`]; captured words and strings use [`AuxData`](crate::ipc::acquisition::AuxData).

use super::procfs::read_bounded_bytes;
use super::{ProcessId, memory, permission_help, timestamp_ms, validate_process_identity};
use anyhow::{Context, Result, bail, ensure};
use std::{fs::File, io::Read};

const MAX_AUXV: usize = 64 * 1024;

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
    let bytes = memory::read_process_memory(pid, address, 4096)?;
    let end = bytes
        .iter()
        .position(|b| *b == 0)
        .context("No string terminator found within 4096 bytes.")?;
    Ok(String::from_utf8_lossy(&bytes[..end]).into_owned())
}

pub(super) fn read_auxiliary_vector(id: ProcessId) -> Result<crate::ipc::acquisition::AuxData> {
    validate_process_identity(id)?;
    let mut ident = [0; 16];
    File::open(format!("/proc/{}/exe", id.pid))?.read_exact(&mut ident)?;
    let width = elf_word_bytes(&ident)?;
    let bytes = read_bounded_bytes(File::open(format!("/proc/{}/auxv", id.pid))?, MAX_AUXV)
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

    #[test]
    fn elf_word_bytes_handles_32_and_64_bits_and_rejects_big_endian() {
        let mut ident = *b"\x7fELF\x02\x01\x01\0\0\0\0\0\0\0\0\0";
        assert_eq!(elf_word_bytes(&ident).unwrap(), 8);
        ident[4] = 1;
        assert_eq!(elf_word_bytes(&ident).unwrap(), 4);
        ident[5] = 2;
        assert!(elf_word_bytes(&ident).is_err());
    }
}
