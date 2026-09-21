pub mod disasm;
pub mod ptrace;
pub mod registers;
pub mod unwind_fp;

use crate::{
    process::{
        self, ProcessId,
        maps::{self, MemoryMap},
    },
    symbol::Symbolizer,
};
use anyhow::{Result, anyhow, ensure};
use serde::Serialize;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Serialize)]
pub struct ThreadSnapshot {
    pub tid: i32,
    pub registers: Vec<registers::Register>,
    pub call_stack: Vec<unwind_fp::StackFrame>,
    pub disassembly: Option<disasm::Disassembly>,
    pub unwind_stop: String,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ProcessSnapshot {
    pub captured_at: u64,
    pub process_id: ProcessId,
    pub paused_ms: f64,
    pub threads: Vec<ThreadSnapshot>,
    pub maps: Vec<MemoryMap>,
}

pub fn capture(id: ProcessId, symbols: Arc<Mutex<Symbolizer>>) -> Result<ProcessSnapshot> {
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
    let mut symbols = symbols.lock().unwrap_or_else(|e| e.into_inner());
    for thread in &mut snapshot.threads {
        if let Some(disassembly) = &mut thread.disassembly {
            disassembly.decode();
        }
        for (index, frame) in thread.call_stack.iter_mut().enumerate() {
            symbols.resolve(id.pid, &snapshot.maps, frame, index > 0);
        }
    }
    Ok(snapshot)
}
