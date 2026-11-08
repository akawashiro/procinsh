//! Decode captured instruction bytes in the unprivileged HTTP process.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`decode_response`] | `pub(super)` | `fn decode_response(response: &mut `[`MonitorResponse`]`) -> `[`Result`]`<()>` |
//!
//! The response envelope is [`MonitorResponse`].

use crate::http_server::analysis::snapshot::architecture::Architecture;
use crate::http_server::protocol::{MonitorEvent, MonitorResponse};
use anyhow::Result;
use capstone::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_BYTES: usize = 256;
const MAX_INSTRUCTIONS: usize = 32;

#[derive(Serialize)]
struct Instruction {
    address: String,
    bytes: Vec<u8>,
    text: String,
    current: bool,
}

#[derive(Deserialize, Serialize)]
struct Disassembly {
    address: String,
    bytes: Vec<u8>,
    #[serde(skip_deserializing)]
    instructions: Vec<Instruction>,
    error: Option<String>,
}

fn append_error_disassembly(state: &mut Disassembly, detail: String) {
    state.error = Some(match state.error.take() {
        Some(error) => format!("{error} {detail}"),
        None => detail,
    });
}

/// Decode from the sampled PC, a known instruction boundary. Invalid bytes
/// stop the decode; skip-data mode is deliberately disabled.
fn decode_disassembly(state: &mut Disassembly, architecture: Option<Architecture>) -> Result<()> {
    let address = u64::from_str_radix(state.address.trim_start_matches("0x"), 16)?;
    state.instructions.clear();
    let Some(architecture) = architecture else {
        append_error_disassembly(
            state,
            "Unsupported or unknown process instruction set.".into(),
        );
        return Ok(());
    };
    if architecture == Architecture::Aarch64 && !address.is_multiple_of(4) {
        append_error_disassembly(state, "Unaligned AArch64 instruction address.".into());
        return Ok(());
    }
    let decoder = match architecture {
        Architecture::X86_64 => Capstone::new()
            .x86()
            .mode(arch::x86::ArchMode::Mode64)
            .syntax(arch::x86::ArchSyntax::Intel)
            .build()?,
        Architecture::Aarch64 => Capstone::new()
            .arm64()
            .mode(arch::arm64::ArchMode::Arm)
            .build()?,
    };
    let bytes = &state.bytes[..state.bytes.len().min(MAX_BYTES)];
    let decoded = decoder.disasm_count(bytes, address, MAX_INSTRUCTIONS)?;
    let mut used = 0;
    for instruction in decoded.iter() {
        used += instruction.bytes().len();
        let mnemonic = instruction.mnemonic().unwrap_or("");
        let operands = instruction.op_str().unwrap_or("");
        let text = if operands.is_empty() {
            mnemonic.into()
        } else {
            format!("{mnemonic} {operands}")
        };
        state.instructions.push(Instruction {
            address: format!("0x{:016x}", instruction.address()),
            bytes: instruction.bytes().to_vec(),
            text,
            current: state.instructions.is_empty(),
        });
    }
    if state.instructions.len() < MAX_INSTRUCTIONS && used < bytes.len() {
        // Capstone does not distinguish invalid x86 encodings from an
        // incomplete instruction. For fixed-width A64 the tail is exact.
        let reason = if architecture == Architecture::Aarch64 && bytes.len() - used < 4 {
            "The captured range ends mid-instruction."
        } else {
            "Invalid, unsupported, or truncated instruction encoding (range may end mid-instruction)."
        };
        append_error_disassembly(
            state,
            format!("0x{:016x}: {reason}", address.wrapping_add(used as u64)),
        );
    }
    Ok(())
}

/// IPC observations carry captured bytes and acquisition errors. Decode only
/// after receipt, before any HTTP serialization, preserving acquisition errors.
pub(super) fn decode_response(response: &mut MonitorResponse) -> Result<()> {
    let value = match response {
        MonitorResponse::Data(value) => value,
        MonitorResponse::Started {
            initial: MonitorEvent::Observation(value),
            ..
        }
        | MonitorResponse::Event {
            event: Some(MonitorEvent::Observation(value)),
            ..
        } => value,
        _ => return Ok(()),
    };
    if let Some(samples) = value.get_mut("live_samples").and_then(Value::as_array_mut) {
        for sample in samples {
            let architecture = sample
                .get("architecture")
                .cloned()
                .map(serde_json::from_value::<Option<Architecture>>)
                .transpose()?
                .flatten();
            if let Some(code) = sample.get_mut("disassembly").filter(|code| !code.is_null()) {
                let mut decoded: Disassembly = serde_json::from_value(code.take())?;
                decode_disassembly(&mut decoded, architecture)?;
                *code = serde_json::to_value(decoded)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_disassembly(address: u64) -> Disassembly {
        Disassembly {
            address: format!("0x{address:016x}"),
            bytes: Vec::new(),
            instructions: Vec::new(),
            error: None,
        }
    }
    #[test]
    fn decodes_initial_and_polled_observations_from_native_instruction_bytes() {
        let code = crate::ipc::acquisition::InstructionBytes {
            address: 0x1000,
            bytes: vec![144, 72, 139],
            error: Some("Partial read.".into()),
        };
        let code: crate::ipc::acquisition::InstructionBytes =
            crate::ipc::codec::decode(&crate::ipc::codec::encode(&code, 4096).unwrap()).unwrap();
        let raw = serde_json::json!({"live_samples": [
            {"tid": 123, "architecture":"x86_64", "disassembly": code},
            {"tid": 124, "disassembly": null},
            {"tid": 125, "architecture":"x86_64", "disassembly": {"address": "0x0000000000002000", "bytes": [], "error": "Read failed."}}
        ]});
        let responses = [
            MonitorResponse::Data(raw.clone()),
            MonitorResponse::Started {
                session: 1,
                initial: MonitorEvent::Observation(raw.clone()),
            },
            MonitorResponse::Event {
                event: Some(MonitorEvent::Observation(raw)),
                closed: false,
            },
        ];
        for mut response in responses {
            decode_response(&mut response).unwrap();
            let value = match response {
                MonitorResponse::Data(value)
                | MonitorResponse::Started {
                    initial: MonitorEvent::Observation(value),
                    ..
                }
                | MonitorResponse::Event {
                    event: Some(MonitorEvent::Observation(value)),
                    ..
                } => value,
                _ => unreachable!(),
            };
            let samples = &value["live_samples"];
            assert_eq!(samples[0]["tid"], 123);
            let code = &samples[0]["disassembly"];
            assert_eq!(code["address"], "0x0000000000001000");
            assert_eq!(code["bytes"], serde_json::json!([144, 72, 139]));
            assert_eq!(
                code["instructions"][0],
                serde_json::json!({"address": "0x0000000000001000", "bytes": [144], "text": "nop", "current": true})
            );
            assert!(code["error"].as_str().unwrap().starts_with("Partial read."));
            assert!(code["error"].as_str().unwrap().contains("mid-instruction"));
            assert!(samples[1]["disassembly"].is_null());
            assert_eq!(samples[2]["disassembly"]["error"], "Read failed.");
            assert_eq!(
                samples[2]["disassembly"]["instructions"],
                serde_json::json!([])
            );
        }
        decode_response(&mut MonitorResponse::Closed).unwrap();
        decode_response(&mut MonitorResponse::Data(
            serde_json::json!({"live_samples": []}),
        ))
        .unwrap();
    }

    #[test]
    fn decodes_variable_length_instructions_and_runtime_branch_target() {
        let mut code = empty_disassembly(0x7fff_1234_5000);
        code.bytes = vec![0x55, 0x48, 0x89, 0xe5, 0xe8, 0x10, 0, 0, 0, 0xc3];
        super::decode_disassembly(&mut code, Some(Architecture::X86_64)).unwrap();
        assert!(code.error.is_none());
        assert_eq!(code.instructions.len(), 4);
        assert_eq!(code.instructions[0].text, "push rbp");
        assert_eq!(code.instructions[1].text, "mov rbp, rsp");
        assert_eq!(code.instructions[1].address, "0x00007fff12345001");
        assert_eq!(code.instructions[2].bytes.len(), 5);
        assert!(code.instructions[2].text.contains("7fff12345019"));
        assert!(code.instructions[0].current);
        assert!(code.instructions[1..].iter().all(|i| !i.current));
        let json = serde_json::to_value(&code).unwrap();
        assert_eq!(json["address"], "0x00007fff12345000");
    }

    #[test]
    fn stops_on_truncated_or_invalid_instructions_and_bounds_output() {
        let mut code = empty_disassembly(0x1000);
        code.bytes = vec![0x90, 0x48, 0x8b];
        super::decode_disassembly(&mut code, Some(Architecture::X86_64)).unwrap();
        assert_eq!(code.instructions.len(), 1);
        assert!(code.error.as_ref().unwrap().contains("mid-instruction"));
        let mut invalid = empty_disassembly(0x1000);
        invalid.bytes = vec![0xf0, 0x90]; // LOCK NOP is invalid.
        super::decode_disassembly(&mut invalid, Some(Architecture::X86_64)).unwrap();
        assert!(invalid.instructions.is_empty());
        assert!(invalid.error.is_some());
        let mut many = empty_disassembly(0x1000);
        many.bytes = vec![0x90; 512];
        super::decode_disassembly(&mut many, Some(Architecture::X86_64)).unwrap();
        assert_eq!(many.instructions.len(), MAX_INSTRUCTIONS);
    }
    #[test]
    fn arm64_decodes_runtime_references_and_rejects_unaligned_and_invalid_bytes() {
        let mut code = empty_disassembly(0x1234);
        // NOP; BL +16; ADR x0, +16; ADRP x0, +4096; RET.
        code.bytes = [
            0xd503201fu32,
            0x94000004,
            0x10000080,
            0xb0000000,
            0xd65f03c0,
        ]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
        super::decode_disassembly(&mut code, Some(Architecture::Aarch64)).unwrap();
        assert!(code.error.is_none(), "{:?}", code.error);
        assert_eq!(code.instructions.len(), 5);
        assert_eq!(code.instructions[0].text, "nop");
        assert_eq!(code.instructions[1].text, "bl #0x1248");
        assert_eq!(code.instructions[2].text, "adr x0, #0x124c");
        assert_eq!(code.instructions[3].text, "adrp x0, #0x2000");
        assert_eq!(code.instructions[4].text, "ret");
        assert!(code.instructions[0].current);
        assert!(code.instructions[1..].iter().all(|i| !i.current));
        code.bytes.push(0);
        super::decode_disassembly(&mut code, Some(Architecture::Aarch64)).unwrap();
        assert!(code.error.as_ref().unwrap().contains("mid-instruction"));
        let mut code = empty_disassembly(0x1001);
        code.bytes = 0xd503201fu32.to_le_bytes().to_vec();
        super::decode_disassembly(&mut code, Some(Architecture::Aarch64)).unwrap();
        assert!(code.instructions.is_empty());
        assert!(code.error.as_ref().unwrap().contains("Unaligned"));
        let mut code = empty_disassembly(0x1000);
        code.bytes = vec![0xff; 4];
        super::decode_disassembly(&mut code, Some(Architecture::Aarch64)).unwrap();
        assert!(code.instructions.is_empty());
        assert!(code.error.is_some());
        let mut code = empty_disassembly(0x1000);
        code.bytes = [0x1f, 0x20, 0x03, 0xd5].repeat(100);
        super::decode_disassembly(&mut code, Some(Architecture::Aarch64)).unwrap();
        assert_eq!(code.instructions.len(), MAX_INSTRUCTIONS);
    }
}
