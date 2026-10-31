//! Identify the instruction set from an acquired executable header.
//!
//! # Interface
//! [`Architecture`] (`pub(in crate::http_server)`): `enum Architecture`.
use serde::{Deserialize, Serialize};

/// 64-bit instruction set of a sampled process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(in crate::http_server) enum Architecture {
    X86_64,
    Aarch64,
}

// ELF specification: e_ident offsets and values, and e_machine numbers.
const ELF_MAGIC: &[u8; 4] = b"\x7fELF";
const ELFCLASS64: u8 = 2;
const ELFDATA2LSB: u8 = 1;
const EM_X86_64: u16 = 62;
const EM_AARCH64: u16 = 183;

impl Architecture {
    /// Classifies an ELF header; 32-bit, big-endian and other machines are unsupported.
    pub(in crate::http_server) fn from_elf_header(header: &[u8; 20]) -> Option<Self> {
        if &header[..4] != ELF_MAGIC || header[4] != ELFCLASS64 || header[5] != ELFDATA2LSB {
            return None;
        }
        match u16::from_le_bytes([header[18], header[19]]) {
            EM_X86_64 => Some(Self::X86_64),
            EM_AARCH64 => Some(Self::Aarch64),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(class: u8, data: u8, machine: u16) -> [u8; 20] {
        let mut header = [0; 20];
        header[..4].copy_from_slice(ELF_MAGIC);
        header[4] = class;
        header[5] = data;
        header[18..].copy_from_slice(&machine.to_le_bytes());
        header
    }

    #[test]
    fn elf_header_identifies_supported_64_bit_machines() {
        assert_eq!(
            Architecture::from_elf_header(&header(ELFCLASS64, ELFDATA2LSB, EM_X86_64)),
            Some(Architecture::X86_64)
        );
        assert_eq!(
            Architecture::from_elf_header(&header(ELFCLASS64, ELFDATA2LSB, EM_AARCH64)),
            Some(Architecture::Aarch64)
        );
        // 32-bit x86 (EM_386), 32-bit ARM (EM_ARM), big-endian and non-ELF files.
        assert_eq!(
            Architecture::from_elf_header(&header(1, ELFDATA2LSB, 3)),
            None
        );
        assert_eq!(
            Architecture::from_elf_header(&header(1, ELFDATA2LSB, 40)),
            None
        );
        assert_eq!(
            Architecture::from_elf_header(&header(ELFCLASS64, 2, EM_AARCH64)),
            None
        );
        assert_eq!(Architecture::from_elf_header(&[0; 20]), None);
        assert_eq!(
            serde_json::to_value([Architecture::X86_64, Architecture::Aarch64]).unwrap(),
            serde_json::json!(["x86_64", "aarch64"])
        );
    }

    #[test]
    fn current_process_matches_the_build_target() {
        let expected = if cfg!(target_arch = "aarch64") {
            Architecture::Aarch64
        } else {
            Architecture::X86_64
        };
        assert_eq!(
            {
                use std::io::Read;
                let mut header = [0; 20];
                std::fs::File::open(format!("/proc/{}/exe", std::process::id()))
                    .unwrap()
                    .read_exact(&mut header)
                    .unwrap();
                Architecture::from_elf_header(&header)
            },
            Some(expected)
        );
    }
}
