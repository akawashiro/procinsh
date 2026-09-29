use super::{
    disasm::{self, Disassembly},
    ptrace,
    registers::{self, Register},
    unwind_fp,
};
use crate::http_server::{
    process::snapshot::stack::StackFrame,
    process::snapshot::symbol::{
        ElfCache, SymbolInfo, elf_address, instruction_address, resolve_frame,
    },
    process::{
        self, ProcessId,
        maps::{self, MemoryMap},
    },
};
use anyhow::{Result, anyhow, ensure};
use serde::Serialize;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize)]
struct ThreadSnapshot {
    tid: i32,
    registers: Vec<Register>,
    call_stack: Vec<StackFrame>,
    disassembly: Option<Disassembly>,
    unwind_stop: String,
    error: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub(in crate::http_server::process) struct ProcessSnapshot {
    captured_at: u64,
    process_id: ProcessId,
    paused_ms: f64,
    threads: Vec<ThreadSnapshot>,
    maps: Vec<MemoryMap>,
}

fn capture(id: ProcessId, symbols: Arc<Mutex<ElfCache>>) -> Result<ProcessSnapshot> {
    let mut snapshot = std::thread::Builder::new()
        .name("snapshot-tracer".into())
        .spawn(move || -> Result<ProcessSnapshot> {
            let start = Instant::now();
            let guard = ptrace::SnapshotGuard::capture(id)?;
            let deadline = Instant::now() + Duration::from_secs(2);
            let captured_at = process::timestamp_ms();
            // Avoid smaps/DWARF parsing while the target is stopped.
            let maps = maps::read(id.pid, false)?;
            let mut threads = Vec::new();
            for tid in guard.tids() {
                ensure!(
                    Instant::now() < deadline,
                    "snapshot capture exceeded the 2-second stop budget"
                );
                match guard.registers(tid) {
                    Ok(raw) => {
                        let (call_stack, unwind_stop) =
                            unwind_fp::capture(id.pid, &raw, &maps, deadline);
                        threads.push(ThreadSnapshot {
                            tid,
                            registers: registers::from_raw(&raw, &maps),
                            call_stack,
                            disassembly: Some(disasm::Disassembly::capture(
                                id.pid, &raw, &maps, deadline,
                            )),
                            unwind_stop,
                            error: None,
                        });
                    }
                    Err(e) => threads.push(ThreadSnapshot {
                        tid,
                        registers: Vec::new(),
                        call_stack: Vec::new(),
                        disassembly: None,
                        unwind_stop: "register read failed".into(),
                        error: Some(e.to_string()),
                    }),
                }
            }
            process::check_identity(id)?;
            drop(guard);
            Ok(ProcessSnapshot {
                captured_at,
                process_id: id,
                paused_ms: start.elapsed().as_secs_f64() * 1000.0,
                threads,
                maps,
            })
        })?
        .join()
        .map_err(|_| anyhow!("snapshot worker panicked; tracer thread exited"))??;
    // Tracer has exited before symbolization: even cleanup failures cannot keep
    // tracees attached while ELF/debug files are being parsed.
    for thread in &mut snapshot.threads {
        if let Some(disassembly) = &mut thread.disassembly {
            disassembly.decode();
        }
        for (index, frame) in thread.call_stack.iter_mut().enumerate() {
            let address = instruction_address(frame.address, index > 0);
            let info = snapshot
                .maps
                .iter()
                .find(|m| m.contains(address) && m.inode != 0)
                .and_then(|map| {
                    let elf = symbols
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .get(id.pid, map)?;
                    let address = elf_address(address, map, &elf, process::procfs::page_size())?;
                    resolve_frame(address, &elf)
                });
            apply_symbol_info(frame, info);
        }
    }
    Ok(snapshot)
}

// Replace all resolved fields so repeated application cannot append inline frames
// or preserve stale data from an earlier resolution.
fn apply_symbol_info(frame: &mut StackFrame, info: Option<SymbolInfo>) {
    let info = info.unwrap_or_default();
    frame.symbol = info.name;
    frame.symbol_offset = info.offset.map(|offset| format!("0x{offset:x}"));
    frame.source_file = info.file;
    frame.line = info.line;
    frame.inline_frames = info.inline_frames;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http_server::process::snapshot::stack::SourceFrame;

    #[test]
    fn applying_symbols_replaces_fields_and_preserves_json_contract() {
        let mut frame = StackFrame::raw(0x1234);
        let info = SymbolInfo {
            name: Some("inner".into()),
            offset: Some(0xa),
            file: Some("test.c".into()),
            line: Some(12),
            inline_frames: vec![SourceFrame {
                function: Some("inner".into()),
                file: Some("test.c".into()),
                line: Some(12),
            }],
        };
        apply_symbol_info(&mut frame, Some(info.clone()));
        let once = frame.clone();
        apply_symbol_info(&mut frame, Some(info));
        assert_eq!(frame, once);
        assert_eq!(
            serde_json::to_value(&frame).unwrap(),
            serde_json::json!({
                "address": "0x0000000000001234", "symbol": "inner", "symbol_offset": "0xa", "source_file": "test.c", "line": 12,
                "inline_frames": [{"function": "inner", "file": "test.c", "line": 12}]
            })
        );
        apply_symbol_info(&mut frame, None);
        assert_eq!(frame, StackFrame::raw(0x1234));
    }
}

#[derive(Default)]
pub(in crate::http_server::process) struct Snapshotter {
    lock: Mutex<()>,
    symbols: Arc<Mutex<ElfCache>>,
}
impl Snapshotter {
    pub(in crate::http_server::process) fn capture(
        &self,
        id: ProcessId,
    ) -> Result<ProcessSnapshot> {
        let _snapshot = self.lock.lock().unwrap();
        process::check_identity(id)?;
        capture(id, self.symbols.clone())
    }
}

#[cfg(test)]
#[path = "fixture_tests.rs"]
mod fixture_tests;
