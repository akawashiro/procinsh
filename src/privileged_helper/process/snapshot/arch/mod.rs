//! Instruction-set identification of the inspected process, read from its ELF header.
use serde::Serialize;
use std::io::Read;

#[cfg(target_arch = "aarch64")]
pub(super) mod aarch64;
#[cfg(target_arch = "x86_64")]
pub(super) mod x86_64;
/// Register capture and naming for the architecture ProcInSh is built for.
#[cfg(target_arch = "aarch64")]
pub(super) use aarch64 as native;
#[cfg(target_arch = "x86_64")]
pub(super) use x86_64 as native;

/// 64-bit instruction set of a sampled process.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Architecture {
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
    fn from_elf_header(header: &[u8; 20]) -> Option<Self> {
        if &header[..4] != ELF_MAGIC || header[4] != ELFCLASS64 || header[5] != ELFDATA2LSB {
            return None;
        }
        match u16::from_le_bytes([header[18], header[19]]) {
            EM_X86_64 => Some(Self::X86_64),
            EM_AARCH64 => Some(Self::Aarch64),
            _ => None,
        }
    }

    /// Reads the executable of `pid`. Kernel threads, unreadable executables and
    /// unsupported ABIs yield `None`; sampling itself does not depend on the result.
    pub(super) fn of_process(pid: i32) -> Option<Self> {
        let mut header = [0; 20];
        std::fs::File::open(format!("/proc/{pid}/exe"))
            .and_then(|mut file| file.read_exact(&mut header))
            .ok()?;
        Self::from_elf_header(&header)
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
            Architecture::of_process(std::process::id() as i32),
            Some(expected)
        );
    }
}
