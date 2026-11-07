//! Capture bounded instruction bytes from the sampled PC for main-side decoding.
//!
//! # Interface
//! No named types or functions are exported. Capture methods are implemented on
//! [`InstructionBytes`], defined in the shared acquisition module.

use crate::{ipc::model::memory::MemoryMap, privileged_helper::process::memory};

const MAX_BYTES: usize = 256;

use crate::ipc::acquisition::InstructionBytes;

impl InstructionBytes {
    fn empty(address: u64) -> Self {
        Self {
            address,
            bytes: Vec::new(),
            error: None,
        }
    }

    /// Best-effort read after sampling; mutable code may have changed.
    pub(super) fn capture(pid: i32, rip: u64, maps: &[MemoryMap]) -> Self {
        let mut result = Self::empty(rip);
        let Some(map) = maps.iter().find(|m| m.contains(rip)) else {
            result.error = Some("PC is outside the memory mappings.".into());
            return result;
        };
        let length = (map.end - rip).min(MAX_BYTES as u64) as usize;
        match memory::read_process_memory(pid, rip, length) {
            Ok(bytes) => {
                if bytes.len() < length {
                    result.error = Some(format!(
                        "Only some instruction bytes were read ({} / {length} bytes).",
                        bytes.len()
                    ));
                }
                result.bytes = bytes;
            }
            Err(error) => {
                result.error = Some(format!("Could not read instruction bytes: {error:#}"))
            }
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use crate::ipc::acquisition::InstructionBytes;

    #[test]
    fn reads_mapping_boundary_and_retains_captured_bytes_after_memory_changes() {
        let mut bytes = [0x55u8, 0xc3];
        let address = bytes.as_ptr() as u64;
        let map = crate::ipc::model::memory::parse_map(&format!(
            "{address:x}-{:x} r-xp 0 00:00 0",
            address + 2
        ))
        .unwrap();
        let code = InstructionBytes::capture(std::process::id() as i32, address, &[map]);
        assert_eq!(code.bytes.len(), 2);
        bytes.fill(0x90);
        assert_eq!(code.bytes, [0x55, 0xc3]);
        let json = serde_json::to_value(&code).unwrap();
        assert!(json.get("instructions").is_none());
        assert!(
            InstructionBytes::capture(std::process::id() as i32, address, &[])
                .error
                .is_some()
        );
    }
}
