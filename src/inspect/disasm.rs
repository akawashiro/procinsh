use crate::process::{maps::MemoryMap, memory};
use iced_x86::{Decoder, DecoderError, DecoderOptions, Formatter, IntelFormatter};
use serde::Serialize;

const MAX_BYTES: usize = 256;
const MAX_INSTRUCTIONS: usize = 32;

#[derive(Clone, Debug, Serialize)]
pub struct Instruction {
    #[serde(serialize_with = "crate::process::maps::hex")]
    pub address: u64,
    pub bytes: Vec<u8>,
    pub text: String,
    pub current: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct Disassembly {
    #[serde(serialize_with = "crate::process::maps::hex")]
    pub address: u64,
    pub bytes: Vec<u8>,
    pub instructions: Vec<Instruction>,
    pub error: Option<String>,
}

impl Disassembly {
    fn empty(address: u64) -> Self {
        Self {
            address,
            bytes: Vec::new(),
            instructions: Vec::new(),
            error: None,
        }
    }

    /// Instruction bytes are live reads, later than the perf sample.
    pub fn capture(pid: i32, address: u64, maps: &[MemoryMap]) -> Self {
        let mut result = Self::empty(address);
        let Some(map) = maps.iter().find(|m| m.contains(address) && m.readable) else {
            result.error = Some("Address is outside readable mappings".into());
            return result;
        };
        let length = (map.end - address).min(MAX_BYTES as u64) as usize;
        match memory::read_raw(pid, address, length) {
            Ok(bytes) => {
                if bytes.len() < length {
                    result.error = Some("Partial instruction read".into());
                }
                result.bytes = bytes;
            }
            Err(error) => {
                result.error = Some(format!("Could not read instruction bytes: {error:#}"))
            }
        }
        result.decode();
        result
    }

    pub fn decode(&mut self) {
        self.instructions.clear();
        let bytes = &self.bytes[..self.bytes.len().min(MAX_BYTES)];
        let mut decoder = Decoder::with_ip(64, bytes, self.address, DecoderOptions::NONE);
        let mut formatter = IntelFormatter::new();
        formatter.options_mut().set_hex_prefix("0x");
        formatter.options_mut().set_hex_suffix("");
        formatter.options_mut().set_uppercase_hex(false);
        for _ in 0..MAX_INSTRUCTIONS {
            if !decoder.can_decode() {
                break;
            }
            let offset = decoder.position();
            let instruction = decoder.decode();
            if instruction.is_invalid() {
                let reason = if decoder.last_error() == DecoderError::NoMoreBytes {
                    "The captured range ends mid-instruction."
                } else {
                    "Invalid or unsupported instruction encoding."
                };
                let detail = format!("0x{:016x}: {reason}", instruction.ip());
                self.error = Some(match self.error.take() {
                    Some(error) => format!("{error} {detail}"),
                    None => detail,
                });
                break;
            }
            let mut text = String::new();
            formatter.format(&instruction, &mut text);
            self.instructions.push(Instruction {
                address: instruction.ip(),
                bytes: bytes[offset..decoder.position()].to_vec(),
                text,
                current: offset == 0,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decodes_variable_length_instructions_and_runtime_branch_target() {
        let mut code = Disassembly::empty(0x7fff_1234_5000);
        code.bytes = vec![0x55, 0x48, 0x89, 0xe5, 0xe8, 0x10, 0, 0, 0, 0xc3];
        code.decode();
        assert!(code.error.is_none());
        assert_eq!(code.instructions.len(), 4);
        assert_eq!(code.instructions[0].text, "push rbp");
        assert_eq!(code.instructions[1].text, "mov rbp,rsp");
        assert_eq!(code.instructions[1].address, code.address + 1);
        assert_eq!(code.instructions[2].bytes.len(), 5);
        assert!(code.instructions[2].text.contains("7fff12345019"));
        assert!(code.instructions[0].current);
        assert!(code.instructions[1..].iter().all(|i| !i.current));
        let json = serde_json::to_value(&code).unwrap();
        assert_eq!(json["address"], "0x00007fff12345000");
    }

    #[test]
    fn stops_on_truncated_or_invalid_instructions_and_bounds_output() {
        let mut code = Disassembly::empty(0x1000);
        code.bytes = vec![0x90, 0x48, 0x8b];
        code.decode();
        assert_eq!(code.instructions.len(), 1);
        assert!(code.error.as_ref().unwrap().contains("mid-instruction"));
        let mut invalid = Disassembly::empty(0x1000);
        invalid.bytes = vec![0xf0, 0x90]; // LOCK NOP is invalid.
        invalid.decode();
        assert!(invalid.instructions.is_empty());
        assert!(invalid.error.is_some());
        let mut many = Disassembly::empty(0x1000);
        many.bytes = vec![0x90; 512];
        many.decode();
        assert_eq!(many.instructions.len(), MAX_INSTRUCTIONS);
    }

    #[test]
    fn reads_mapping_boundary() {
        let bytes = [0x55u8, 0xc3];
        let address = bytes.as_ptr() as u64;
        let map = crate::process::maps::parse_map(&format!(
            "{address:x}-{:x} r-xp 0 00:00 0",
            address + 2
        ))
        .unwrap();
        let code = Disassembly::capture(std::process::id() as i32, address, &[map]);
        assert_eq!(code.bytes.len(), 2);
        assert_eq!(code.instructions[0].text, "push rbp");
        assert_eq!(code.instructions[1].text, "ret");
        assert!(
            Disassembly::capture(std::process::id() as i32, address, &[])
                .error
                .is_some()
        );
    }
}
