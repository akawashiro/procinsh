use super::{
    disasm::Disassembly,
    perf::Event,
    registers::{self, Register},
    stack::StackFrame,
    symbol::{ElfCache, SymbolInfo, elf_address, instruction_address, resolve_frame},
    unwind_fp,
};
use crate::http_server::process::{self, ProcessId, maps::MemoryMap, procfs};
use anyhow::Result;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize)]
pub(in crate::http_server::process) struct ThreadSample {
    tid: i32,
    sampled_at: Option<u64>,
    sample_age_ms: Option<u64>,
    cpu: Option<u32>,
    lost_samples: u64,
    registers: Vec<Register>,
    call_stack: Vec<StackFrame>,
    disassembly: Option<Disassembly>,
    unwind_stop: String,
    error: Option<String>,
}
impl ThreadSample {
    fn waiting(tid: i32) -> Self {
        Self {
            tid,
            sampled_at: None,
            sample_age_ms: None,
            cpu: None,
            lost_samples: 0,
            registers: Vec::new(),
            call_stack: Vec::new(),
            disassembly: None,
            unwind_stop: "Waiting for sample".into(),
            error: None,
        }
    }
}
struct Thread {
    start_time: u64,
    event: Option<Event>,
    latest: ThreadSample,
    sampled_ns: Option<u64>,
}
pub(in crate::http_server::process) struct Sampler {
    id: ProcessId,
    threads: BTreeMap<i32, Thread>,
    symbols: ElfCache,
}
impl Sampler {
    pub(in crate::http_server::process) fn new(id: ProcessId) -> Self {
        Self {
            id,
            threads: BTreeMap::new(),
            symbols: ElfCache::default(),
        }
    }
    pub(in crate::http_server::process) fn poll(&mut self, maps: &[MemoryMap]) -> Result<()> {
        process::check_identity(self.id)?;
        let mut alive = BTreeSet::new();
        for entry in std::fs::read_dir(format!("/proc/{}/task", self.id.pid))? {
            let entry = entry?;
            let Some(tid) = entry
                .file_name()
                .to_str()
                .and_then(|s| s.parse::<i32>().ok())
            else {
                continue;
            };
            let Ok(stat) = procfs::read_stat(&format!("/proc/{}/task/{tid}/stat", self.id.pid))
            else {
                continue;
            };
            alive.insert(tid);
            if self
                .threads
                .get(&tid)
                .is_some_and(|thread| thread.start_time != stat.start_time)
            {
                self.threads.remove(&tid);
            }
            let thread = self.threads.entry(tid).or_insert_with(|| {
                let mut latest = ThreadSample::waiting(tid);
                let event = match Event::open(tid) {
                    Ok(event) => Some(event),
                    Err(e) => {
                        latest.error = Some(format!("{e:#}"));
                        None
                    }
                };
                Thread {
                    start_time: stat.start_time,
                    event,
                    latest,
                    sampled_ns: None,
                }
            });
            let Some(event) = &mut thread.event else {
                continue;
            };
            match event.drain() {
                Ok(Some(sample)) if sample.tid == tid => {
                    let r = &sample.registers;
                    let (mut frames, reason) =
                        unwind_fp::walk(r.0[8], r.0[7], r.0[6], maps, |bp| {
                            let offset = usize::try_from(bp.checked_sub(r.0[7])?).ok()?;
                            sample
                                .stack
                                .get(offset..offset.checked_add(16)?)?
                                .try_into()
                                .ok()
                        });
                    for (index, frame) in frames.iter_mut().enumerate() {
                        let address = instruction_address(frame.address, index > 0);
                        let info = maps
                            .iter()
                            .find(|m| m.contains(address) && m.inode != 0)
                            .and_then(|map| {
                                let elf = self.symbols.get(self.id.pid, map)?;
                                resolve_frame(
                                    elf_address(address, map, &elf, procfs::page_size())?,
                                    &elf,
                                )
                            });
                        apply_symbol_info(frame, info);
                    }
                    let mut code = Disassembly::capture(self.id.pid, r.0[8], maps);
                    code.decode();
                    let age = monotonic_ns().saturating_sub(sample.time_ns) / 1_000_000;
                    thread.sampled_ns = Some(sample.time_ns);
                    thread.latest = ThreadSample {
                        tid,
                        sampled_at: Some(process::timestamp_ms().saturating_sub(age)),
                        sample_age_ms: Some(age),
                        cpu: Some(sample.cpu),
                        lost_samples: event.lost,
                        registers: registers::from_sample(r, maps),
                        call_stack: frames,
                        disassembly: Some(code),
                        unwind_stop: reason,
                        error: None,
                    };
                }
                Ok(_) => {}
                Err(e) => {
                    thread.latest.error = Some(format!("{e:#}"));
                }
            }
            thread.latest.lost_samples = event.lost;
        }
        self.threads.retain(|tid, _| alive.contains(tid));
        process::check_identity(self.id)?;
        Ok(())
    }
    pub(in crate::http_server::process) fn latest(&self) -> Vec<ThreadSample> {
        let now = monotonic_ns();
        self.threads
            .values()
            .map(|thread| {
                let mut latest = thread.latest.clone();
                latest.sample_age_ms = thread
                    .sampled_ns
                    .map(|time| now.saturating_sub(time) / 1_000_000);
                latest
            })
            .collect()
    }
}
fn monotonic_ns() -> u64 {
    let mut time: libc::timespec = unsafe { std::mem::zeroed() };
    unsafe {
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut time);
    }
    time.tv_sec as u64 * 1_000_000_000 + time.tv_nsec as u64
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

#[cfg(test)]
mod live_tests {
    use super::*;
    use crate::http_server::process::TestTarget;
    use std::time::{Duration, Instant};

    fn perf_available(tid: i32) -> bool {
        match Event::open(tid) {
            Ok(_) => true,
            Err(e) => {
                let denied = e.downcast_ref::<std::io::Error>().is_some_and(|error| {
                    matches!(
                        error.raw_os_error(),
                        Some(libc::EPERM | libc::EACCES | libc::ENOSYS)
                    )
                });
                assert!(
                    denied && std::env::var_os("PROCINSH_REQUIRE_PERF").is_none(),
                    "{e:#}"
                );
                eprintln!(
                    "Skipping live perf fixture: {e}; set PROCINSH_REQUIRE_PERF=1 to require it"
                );
                false
            }
        }
    }

    fn poll_until(sampler: &mut Sampler, maps: &[MemoryMap], condition: impl Fn(&Sampler) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            sampler.poll(maps).unwrap();
            if condition(sampler) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("no expected live sample: {:?}", sampler.latest());
    }

    #[test]
    fn live_busy_and_recursive_samples_advance_without_tracing() {
        for name in ["busy_loop", "recursive"] {
            let target = TestTarget::new(name);
            if !perf_available(target.id.pid) {
                return;
            }
            let maps = process::maps::read(target.id.pid, false).unwrap();
            let mut sampler = Sampler::new(target.id);
            poll_until(&mut sampler, &maps, |s| {
                s.latest().iter().any(|t| t.sampled_at.is_some())
            });
            let first = sampler.latest()[0].clone();
            assert_eq!(first.registers.len(), 18);
            assert!(!first.disassembly.as_ref().unwrap().instructions.is_empty());
            if name == "recursive" {
                assert!(first.call_stack.len() >= 4, "{:?}", first.call_stack);
                assert!(
                    first
                        .call_stack
                        .iter()
                        .any(|f| f.symbol.as_deref() == Some("foo"))
                );
            }
            poll_until(&mut sampler, &maps, |s| {
                s.latest()[0].sampled_at > first.sampled_at
            });
            target.assert_detached();
            drop(sampler);
            target.assert_detached();
        }
    }

    #[test]
    fn live_threads_churn_sleep_and_exit() {
        let mut target = TestTarget::new("threads");
        if !perf_available(target.id.pid) {
            return;
        }
        let maps = process::maps::read(target.id.pid, false).unwrap();
        let mut sampler = Sampler::new(target.id);
        poll_until(&mut sampler, &maps, |s| {
            s.latest().iter().filter(|t| t.sampled_at.is_some()).count() >= 4
        });
        let main = sampler
            .latest()
            .into_iter()
            .find(|t| t.tid == target.id.pid)
            .unwrap();
        // Sleeping main is allowed to have no sample; busy workers continue.
        let worker = sampler
            .latest()
            .into_iter()
            .find(|t| t.sampled_at.is_some())
            .unwrap();
        poll_until(&mut sampler, &maps, |s| {
            s.latest()
                .iter()
                .any(|t| t.tid == worker.tid && t.sampled_at > worker.sampled_at)
        });
        let sleeping = sampler
            .latest()
            .into_iter()
            .find(|t| t.tid == main.tid)
            .unwrap();
        assert_eq!(sleeping.sampled_at, main.sampled_at);
        // Short-lived TIDs are removed, and newly created TIDs get their own event.
        poll_until(&mut sampler, &maps, |s| s.threads.len() == 7);
        let old_tids: BTreeSet<_> = sampler.threads.keys().copied().collect();
        poll_until(&mut sampler, &maps, |s| {
            old_tids.iter().any(|tid| !s.threads.contains_key(tid))
                && s.threads.keys().any(|tid| !old_tids.contains(tid))
        });
        assert!(sampler.threads.len() <= 7);
        let sleeper_tid = *sampler
            .threads
            .keys()
            .find(|tid| {
                std::fs::read_to_string(format!("/proc/{}/task/{tid}/comm", target.id.pid))
                    .is_ok_and(|name| name.trim() == "procinsh-sleep")
            })
            .unwrap();
        assert_eq!(unsafe { libc::kill(target.id.pid, libc::SIGUSR1) }, 0);
        std::thread::sleep(Duration::from_millis(300));
        sampler.poll(&maps).unwrap();
        let sleeping_at = sampler.threads[&sleeper_tid].latest.sampled_at;
        assert!(sleeping_at.is_some());
        let busy_at = sampler.threads[&worker.tid].latest.sampled_at;
        std::thread::sleep(Duration::from_millis(400));
        sampler.poll(&maps).unwrap();
        let sleeper = sampler
            .latest()
            .into_iter()
            .find(|t| t.tid == sleeper_tid)
            .unwrap();
        assert_eq!(sleeper.sampled_at, sleeping_at);
        assert!(sleeper.sample_age_ms.unwrap() >= 400);
        assert!(
            sampler
                .latest()
                .iter()
                .any(|t| t.tid != sleeper_tid && t.sampled_at > busy_at)
        );
        target.assert_detached();
        let stale = ProcessId {
            start_time_ticks: target.id.start_time_ticks + 1,
            ..target.id
        };
        assert!(Sampler::new(stale).poll(&maps).is_err());
        target.child.kill().unwrap();
        target.child.wait().unwrap();
        assert!(sampler.poll(&maps).is_err());
        drop(sampler);
    }
}
