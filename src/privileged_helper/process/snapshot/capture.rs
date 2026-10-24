#[cfg(test)]
use super::sample::SampleSource;
use super::sample::monotonic_ns;
use super::{
    arch::{Architecture, native},
    disasm::Disassembly,
    perf::PerfEventReader,
    registers::{self, Register},
    stack::StackFrame,
    symbol::{ElfCache, SymbolInfo, elf_address, instruction_address, resolve_frame},
    unwind::StackUnwinder,
};
use crate::privileged_helper::process::{self, ProcessId, maps::MemoryMap, procfs};
use anyhow::Result;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

const PTRACE_INTERVAL: Duration = Duration::from_secs(10);

/// Latest thread sample with loss and error diagnostics.
/// `sampled_at` is Unix milliseconds; `sample_age_ms` uses monotonic time.
#[derive(Clone, Debug, Serialize)]
pub(in crate::privileged_helper::process) struct ThreadSample {
    tid: i32,
    /// Instruction set of the inspected process; `null` when it is unknown or unsupported.
    architecture: Option<Architecture>,
    sampled_at: Option<u64>,
    #[cfg(test)]
    #[serde(skip)]
    sample_source: Option<SampleSource>,
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
    fn waiting(tid: i32, architecture: Option<Architecture>) -> Self {
        Self {
            tid,
            architecture,
            sampled_at: None,
            #[cfg(test)]
            sample_source: None,
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
    event: Option<PerfEventReader>,
    switch_event: Option<PerfEventReader>,
    setup_error: Option<String>,
    ptrace_error: Option<String>,
    latest: ThreadSample,
    sampled_ns: Option<u64>,
}
pub(in crate::privileged_helper::process) struct Sampler {
    id: ProcessId,
    architecture: Option<Architecture>,
    threads: BTreeMap<i32, Thread>,
    symbols: ElfCache,
    unwind: StackUnwinder,
    next_ptrace: Option<Instant>,
}
impl Sampler {
    pub(in crate::privileged_helper::process) fn new(id: ProcessId) -> Self {
        Self {
            id,
            architecture: Architecture::of_process(id.pid),
            threads: BTreeMap::new(),
            symbols: ElfCache::default(),
            unwind: StackUnwinder::default(),
            next_ptrace: None,
        }
    }
    /// Captures the initial ptrace snapshot and schedules the next attempt ten seconds
    /// after completion, including failed capture attempts.
    pub(in crate::privileged_helper::process) fn bootstrap(
        &mut self,
        maps: &[MemoryMap],
    ) -> Result<()> {
        // Discover all threads and open perf events before interrupting any thread.
        self.next_ptrace = None;
        self.poll(maps)?;
        self.capture_ptrace(maps)
    }
    fn capture_ptrace(&mut self, maps: &[MemoryMap]) -> Result<()> {
        for (&tid, thread) in &mut self.threads {
            thread.ptrace_error = None;
            thread.latest.error = thread.setup_error.clone();
            match super::ptrace::capture(tid, maps) {
                Ok(sample) => process_sample(
                    self.id.pid,
                    self.architecture,
                    &mut self.unwind,
                    &mut self.symbols,
                    thread,
                    sample,
                    maps,
                ),
                Err(error) => {
                    let message = format!("ptrace sampling: {error:#}");
                    thread.ptrace_error = Some(message.clone());
                    thread.latest.error = Some(match thread.latest.error.take() {
                        Some(error) => format!("{error}; {message}"),
                        None => message,
                    });
                }
            }
        }
        // Schedule from completion, including failed attempts; never catch up in a burst.
        self.next_ptrace = Some(Instant::now() + PTRACE_INTERVAL);
        process::validate_process_identity(self.id)
    }
    /// Drains perf samples and captures current threads when the ptrace deadline is due.
    pub(in crate::privileged_helper::process) fn poll(&mut self, maps: &[MemoryMap]) -> Result<()> {
        process::validate_process_identity(self.id)?;
        self.unwind.refresh(self.id.pid, maps, &mut self.symbols);
        let mut alive = BTreeSet::new();
        for tid in super::super::threads::tids(self.id.pid)? {
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
                let mut latest = ThreadSample::waiting(tid, self.architecture);
                let event = match PerfEventReader::open(tid) {
                    Ok(event) => Some(event),
                    Err(e) => {
                        latest.error = Some(format!("{e:#}"));
                        None
                    }
                };
                let switch_event = match PerfEventReader::context_switch(tid) {
                    Ok(event) => Some(event),
                    Err(e) => {
                        let message = format!("context-switch perf: {e:#}");
                        latest.error = Some(match latest.error.take() {
                            Some(error) => format!("{error}; {message}"),
                            None => message,
                        });
                        None
                    }
                };
                Thread {
                    switch_event,
                    setup_error: latest.error.clone(),
                    ptrace_error: None,
                    start_time: stat.start_time,
                    event,
                    latest,
                    sampled_ns: None,
                }
            });
            let mut newest = None;
            let mut lost = 0u64;
            thread.latest.error = match (&thread.setup_error, &thread.ptrace_error) {
                (Some(perf), Some(ptrace)) => Some(format!("{perf}; {ptrace}")),
                (perf, ptrace) => perf.clone().or_else(|| ptrace.clone()),
            };
            for event in [&mut thread.event, &mut thread.switch_event]
                .into_iter()
                .flatten()
            {
                match event.drain() {
                    Ok(Some(sample)) if sample.tid == tid => {
                        if newest
                            .as_ref()
                            .is_none_or(|old: &super::sample::RawSample| {
                                sample.time_ns > old.time_ns
                            })
                        {
                            newest = Some(sample);
                        }
                    }
                    Ok(_) => {}
                    Err(e) => thread.latest.error = Some(format!("{e:#}")),
                }
                lost = lost.saturating_add(event.lost);
            }
            thread.latest.lost_samples = lost;
            if let Some(sample) = newest
                && thread.sampled_ns.is_none_or(|time| sample.time_ns > time)
            {
                process_sample(
                    self.id.pid,
                    self.architecture,
                    &mut self.unwind,
                    &mut self.symbols,
                    thread,
                    sample,
                    maps,
                );
            }
        }
        self.threads.retain(|tid, _| alive.contains(tid));
        if self
            .next_ptrace
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            self.capture_ptrace(maps)?;
        }
        process::validate_process_identity(self.id)?;
        Ok(())
    }
    /// Returns the latest samples with ages refreshed from monotonic time.
    pub(in crate::privileged_helper::process) fn latest(&self) -> Vec<ThreadSample> {
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
fn process_sample(
    pid: i32,
    architecture: Option<Architecture>,
    unwind: &mut StackUnwinder,
    symbols: &mut ElfCache,
    thread: &mut Thread,
    sample: super::sample::RawSample,
    maps: &[MemoryMap],
) {
    if thread.sampled_ns.is_some_and(|time| sample.time_ns <= time) {
        return;
    }
    let tid = sample.tid;
    let lost = thread.latest.lost_samples;
    let r = &sample.registers;
    let (mut frames, reason) = unwind.walk(
        r.0[native::PC],
        r.0[native::SP],
        r.0[native::FP],
        maps,
        &sample.stack,
    );
    for (index, frame) in frames.iter_mut().enumerate() {
        let address = instruction_address(frame.address, index > 0);
        let info = maps
            .iter()
            .find(|m| m.contains(address) && m.inode != 0)
            .and_then(|map| {
                let elf = symbols.get(pid, map)?;
                resolve_frame(elf_address(address, map, &elf, procfs::page_size())?, &elf)
            });
        apply_symbol_info(frame, info);
    }
    let mut code = Disassembly::capture(pid, r.0[native::PC], maps);
    code.decode();
    let age = monotonic_ns().saturating_sub(sample.time_ns) / 1_000_000;
    thread.sampled_ns = Some(sample.time_ns);
    thread.latest = ThreadSample {
        tid,
        architecture,
        sampled_at: Some(process::timestamp_ms().saturating_sub(age)),
        #[cfg(test)]
        sample_source: Some(sample.source),
        sample_age_ms: Some(age),
        cpu: sample.cpu,
        lost_samples: lost,
        registers: registers::from_sample(r, maps),
        call_stack: frames,
        disassembly: Some(code),
        unwind_stop: reason,
        error: thread.latest.error.clone(),
    };
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
    use crate::privileged_helper::process::snapshot::stack::SourceFrame;

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
    use crate::privileged_helper::process::TestTarget;
    use std::{
        io::{BufRead, BufReader, Write},
        os::{fd::OwnedFd, unix::net::UnixStream},
        process::{Command, Stdio},
        time::{Duration, Instant},
    };

    /// Commands one worker at a time and waits for readiness or completed join.
    struct ThreadController {
        target: TestTarget,
        connection: BufReader<UnixStream>,
    }

    impl ThreadController {
        fn new() -> Self {
            crate::privileged_helper::test_support::build_targets();
            let (connection, child_connection) = UnixStream::pair().unwrap();
            connection
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            connection
                .set_write_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let stdin: OwnedFd = child_connection.try_clone().unwrap().into();
            let stdout: OwnedFd = child_connection.into();
            let child = Command::new("tests/targets/bin/thread_lifecycle")
                .arg("--allow-inspector")
                .stdin(Stdio::from(stdin))
                .stdout(Stdio::from(stdout))
                .spawn()
                .unwrap();
            let id = process::identity(child.id() as i32).unwrap();
            let mut controller = Self {
                target: TestTarget {
                    child,
                    id,
                    address: 0,
                },
                connection: BufReader::new(connection),
            };
            assert_eq!(controller.response().trim(), "ready");
            controller
        }

        fn response(&mut self) -> String {
            let mut response = String::new();
            assert!(
                self.connection.read_line(&mut response).unwrap() > 0,
                "lifecycle fixture closed its control connection"
            );
            response
        }

        fn command(&mut self, command: &str) -> i32 {
            writeln!(self.connection.get_mut(), "{command}").unwrap();
            self.response().trim().parse().unwrap()
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
    fn live_sleeping_bootstrap_and_perf_handoff() {
        let target = TestTarget::new("sleeping");
        let maps = process::maps::read_maps(target.id.pid).unwrap();
        let mut sampler = Sampler::new(target.id);
        sampler.bootstrap(&maps).unwrap();
        let first = sampler.latest()[0].clone();
        assert_eq!(first.sample_source, Some(SampleSource::Ptrace), "{first:?}");
        assert_eq!(first.architecture, Some(Architecture::X86_64));
        assert_eq!(first.registers.len(), 18);
        assert!(first.cpu.is_none());
        assert!(
            first
                .call_stack
                .iter()
                .any(|frame| frame.symbol.as_deref() == Some("main")),
            "{:?}",
            first.call_stack
        );
        target.assert_detached();
        PerfEventReader::open(target.id.pid)
            .expect("perf access is required in the Docker test container");
        poll_until(&mut sampler, &maps, |s| {
            s.latest()[0].sampled_at > first.sampled_at
                && s.latest()[0].sample_source != Some(SampleSource::Ptrace)
        });
        target.assert_detached();
        // The periodic capture also hands control back to real perf updates.
        sampler.next_ptrace = Some(Instant::now());
        sampler.poll(&maps).unwrap();
        assert_eq!(
            sampler.latest()[0].sample_source,
            Some(SampleSource::Ptrace)
        );
        let periodic_ns = sampler.threads[&target.id.pid].sampled_ns;
        poll_until(&mut sampler, &maps, |s| {
            s.threads[&target.id.pid].sampled_ns > periodic_ns
                && s.latest()[0].sample_source != Some(SampleSource::Ptrace)
        });
        target.assert_detached();
        drop(sampler);
        target.assert_detached();
    }

    #[test]
    fn periodic_ptrace_waits_for_deadline_and_refreshes_sleeping_thread() {
        let target = TestTarget::new("sleeping");
        let maps = process::maps::read_maps(target.id.pid).unwrap();
        let mut sampler = Sampler::new(target.id);
        sampler.bootstrap(&maps).unwrap();
        let deadline = sampler.next_ptrace.unwrap();
        assert!(deadline.duration_since(Instant::now()) > Duration::from_secs(9));
        // Isolate the periodic source from interrupt-induced context-switch samples.
        let thread = sampler.threads.get_mut(&target.id.pid).unwrap();
        thread.event = None;
        thread.switch_event = None;
        let first = thread.sampled_ns;
        sampler.poll(&maps).unwrap();
        assert_eq!(sampler.threads[&target.id.pid].sampled_ns, first);
        assert_eq!(sampler.next_ptrace, Some(deadline));
        sampler.next_ptrace = Some(Instant::now() - Duration::from_secs(30));
        sampler.poll(&maps).unwrap();
        let thread = &sampler.threads[&target.id.pid];
        assert!(thread.sampled_ns > first);
        assert_eq!(thread.latest.sample_source, Some(SampleSource::Ptrace));
        assert!(thread.ptrace_error.is_none());
        assert_eq!(thread.latest.error, thread.setup_error);
        assert!(
            sampler.next_ptrace.unwrap().duration_since(Instant::now()) > Duration::from_secs(9)
        );
        let second = thread.sampled_ns;
        sampler.poll(&maps).unwrap();
        assert_eq!(sampler.threads[&target.id.pid].sampled_ns, second);
        target.assert_detached();
    }

    #[test]
    fn periodic_ptrace_captures_all_threads_including_newly_discovered_threads() {
        let target = TestTarget::new("threads");
        let maps = process::maps::read_maps(target.id.pid).unwrap();
        let mut sampler = Sampler::new(target.id);
        sampler.bootstrap(&maps).unwrap();
        // This fixture also churns short-lived threads; only its main and four
        // permanent workers are guaranteed to survive the entire capture sweep.
        let stable_tids: Vec<_> = sampler
            .threads
            .keys()
            .copied()
            .filter(|&tid| {
                tid == target.id.pid
                    || std::fs::read_to_string(format!("/proc/{}/task/{tid}/comm", target.id.pid))
                        .is_ok_and(|name| {
                            matches!(name.trim(), "procinsh-worker" | "procinsh-sleep")
                        })
            })
            .collect();
        assert_eq!(stable_tids.len(), 5);
        let old: BTreeMap<_, _> = sampler
            .threads
            .iter()
            .map(|(&tid, thread)| (tid, thread.sampled_ns))
            .collect();
        // Rediscovery follows the same path as a thread born after bootstrap.
        let rediscovered = target.id.pid;
        sampler.threads.remove(&rediscovered);
        sampler.next_ptrace = Some(Instant::now());
        sampler.poll(&maps).unwrap();
        assert!(sampler.threads.contains_key(&rediscovered));
        for tid in stable_tids {
            let thread = &sampler.threads[&tid];
            assert_eq!(thread.latest.sample_source, Some(SampleSource::Ptrace));
            assert!(thread.ptrace_error.is_none());
            assert_eq!(thread.latest.error, thread.setup_error);
            if let Some(previous) = old.get(&tid) {
                assert!(thread.sampled_ns > *previous);
            }
        }
        target.assert_detached();
    }

    #[test]
    fn periodic_ptrace_failure_preserves_sample_and_retries_only_when_due() {
        let target = TestTarget::new("sleeping");
        let maps = process::maps::read_maps(target.id.pid).unwrap();
        let mut sampler = Sampler::new(target.id);
        sampler.bootstrap(&maps).unwrap();
        let thread = sampler.threads.get_mut(&target.id.pid).unwrap();
        thread.event = None;
        thread.switch_event = None;
        let first = thread.sampled_ns;
        // An existing tracer forces SEIZE to fail without changing the last sample.
        assert_eq!(
            unsafe { libc::ptrace(libc::PTRACE_SEIZE, target.id.pid, 0, 0) },
            0
        );
        assert_eq!(
            unsafe { libc::ptrace(libc::PTRACE_INTERRUPT, target.id.pid, 0, 0) },
            0
        );
        let mut status = 0;
        assert_eq!(
            unsafe { libc::waitpid(target.id.pid, &mut status, libc::__WALL) },
            target.id.pid
        );
        assert!(libc::WIFSTOPPED(status));
        sampler.next_ptrace = Some(Instant::now());
        sampler.poll(&maps).unwrap();
        let failed_deadline = sampler.next_ptrace.unwrap();
        let thread = &sampler.threads[&target.id.pid];
        assert_eq!(thread.sampled_ns, first);
        assert!(
            thread
                .latest
                .error
                .as_deref()
                .unwrap()
                .contains("ptrace sampling")
        );
        assert!(failed_deadline.duration_since(Instant::now()) > Duration::from_secs(9));
        assert_eq!(
            unsafe { libc::ptrace(libc::PTRACE_DETACH, target.id.pid, 0, 0) },
            0
        );
        target.assert_detached();
        sampler.poll(&maps).unwrap();
        assert_eq!(sampler.next_ptrace, Some(failed_deadline));
        assert_eq!(sampler.threads[&target.id.pid].sampled_ns, first);
        assert!(sampler.threads[&target.id.pid].latest.error.is_some());
        sampler.next_ptrace = Some(Instant::now());
        sampler.poll(&maps).unwrap();
        let thread = &sampler.threads[&target.id.pid];
        assert!(thread.sampled_ns > first);
        assert!(thread.ptrace_error.is_none());
        assert_eq!(thread.latest.error, thread.setup_error);
        assert_eq!(thread.latest.sample_source, Some(SampleSource::Ptrace));
        target.assert_detached();
    }

    #[test]
    fn sample_ordering_preserves_bootstrap_then_accepts_newer_perf() {
        let target = TestTarget::new("sleeping");
        let maps = process::maps::read_maps(target.id.pid).unwrap();
        let mut sampler = Sampler::new(target.id);
        sampler.bootstrap(&maps).unwrap();
        let thread = sampler.threads.get_mut(&target.id.pid).unwrap();
        let time = thread.sampled_ns.unwrap();
        let sample = super::super::sample::RawSample {
            tid: target.id.pid,
            time_ns: time.saturating_sub(1),
            cpu: Some(0),
            registers: registers::RegisterSet([0; 24]),
            stack: Vec::new(),
            source: SampleSource::CpuClock,
        };
        process_sample(
            target.id.pid,
            sampler.architecture,
            &mut sampler.unwind,
            &mut sampler.symbols,
            thread,
            sample,
            &maps,
        );
        assert_eq!(thread.sampled_ns, Some(time));
        assert_eq!(thread.latest.sample_source, Some(SampleSource::Ptrace));
        let sample = super::super::sample::RawSample {
            tid: target.id.pid,
            time_ns: time + 1,
            cpu: Some(7),
            registers: registers::RegisterSet([0; 24]),
            stack: Vec::new(),
            source: SampleSource::CpuClock,
        };
        process_sample(
            target.id.pid,
            sampler.architecture,
            &mut sampler.unwind,
            &mut sampler.symbols,
            thread,
            sample,
            &maps,
        );
        assert_eq!(thread.sampled_ns, Some(time + 1));
        assert_eq!(thread.latest.sample_source, Some(SampleSource::CpuClock));
        assert_eq!(thread.latest.cpu, Some(7));
        target.assert_detached();
    }

    #[test]
    fn live_blocked_samples_refresh_known_registers_and_bound_deep_stacks() {
        let target = TestTarget::new("blocked_stack");
        PerfEventReader::open(target.id.pid)
            .expect("perf access is required in the Docker test container");
        PerfEventReader::context_switch(target.id.pid).unwrap();
        let maps = process::maps::read_maps(target.id.pid).unwrap();
        let mut sampler = Sampler::new(target.id);
        let mut complete = false;
        let mut truncated = false;
        let mut previous = None;
        let mut updates = 0;
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            sampler.poll(&maps).unwrap();
            let sample = &sampler.latest()[0];
            if sample.sample_source == Some(SampleSource::ContextSwitch { preempted: false }) {
                assert!(sample.error.is_none(), "{:?}", sample.error);
                for (name, expected) in [
                    ("R12", 0x12345678u64),
                    ("R13", 0x23456789),
                    ("R14", 0x3456789a),
                    ("R15", 0x456789ab),
                ] {
                    let register = sample.registers.iter().find(|r| r.name == name).unwrap();
                    assert_eq!(register.value, expected);
                }
                assert!(
                    sample
                        .call_stack
                        .iter()
                        .any(|f| f.symbol.as_deref() == Some("blocked_leaf"))
                );
                complete |= sample
                    .call_stack
                    .iter()
                    .any(|f| f.symbol.as_deref() == Some("main"));
                truncated |= sample.unwind_stop.contains("stack snapshot exhausted");
                if sample.sampled_at != previous {
                    updates += 1;
                    previous = sample.sampled_at;
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            complete && truncated && updates >= 5,
            "complete={complete} truncated={truncated} updates={updates}: {:?}",
            sampler.latest()
        );
        eprintln!(
            "Blocked fixture: {updates} distinct updates in 3s; R12–R15 matched; complete and bounded deep stacks verified"
        );
        target.assert_detached();
    }

    #[test]
    fn live_busy_and_recursive_samples_advance_without_tracing() {
        for name in [
            "busy_loop",
            "recursive",
            "recursive_no_fp",
            "recursive_no_fp_nopie",
        ] {
            let target = TestTarget::new(name);
            PerfEventReader::open(target.id.pid)
                .expect("perf access is required in the Docker test container");
            let maps = process::maps::read_maps(target.id.pid).unwrap();
            let mut sampler = Sampler::new(target.id);
            poll_until(&mut sampler, &maps, |s| {
                s.latest().iter().any(|t| t.sampled_at.is_some())
            });
            let first = sampler.latest()[0].clone();
            assert_eq!(first.registers.len(), 18);
            assert!(!first.disassembly.as_ref().unwrap().instructions.is_empty());
            if name.starts_with("recursive") {
                assert!(first.call_stack.len() >= 4, "{:?}", first.call_stack);
                assert!(
                    first
                        .call_stack
                        .iter()
                        .any(|f| f.symbol.as_deref() == Some("main")),
                    "{:?}",
                    first.call_stack
                );
                assert!(
                    first
                        .call_stack
                        .iter()
                        .any(|f| f.source_file.is_some() && f.line.is_some())
                );
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
    fn live_no_fp_unwinds_across_shared_library_and_libc() {
        let target = TestTarget::new("shared_no_fp");
        PerfEventReader::open(target.id.pid)
            .expect("perf access is required in the Docker test container");
        PerfEventReader::context_switch(target.id.pid).unwrap();
        let maps = process::maps::read_maps(target.id.pid).unwrap();
        let mut sampler = Sampler::new(target.id);
        poll_until(&mut sampler, &maps, |s| {
            let latest = s.latest();
            latest.iter().any(|sample| {
                sample
                    .call_stack
                    .iter()
                    .filter(|f| f.symbol.as_deref() == Some("shared_recurse"))
                    .count()
                    >= 13
                    && sample
                        .call_stack
                        .iter()
                        .any(|f| f.symbol.as_deref() == Some("main"))
                    && sample.call_stack.iter().any(|f| {
                        maps.iter().any(|map| {
                            map.contains(f.address)
                                && map
                                    .pathname
                                    .as_deref()
                                    .is_some_and(|p| p.contains("libc.so"))
                        })
                    })
            })
        });
        target.assert_detached();
    }

    #[test]
    fn live_thread_lifecycle_discovers_and_removes_acknowledged_threads() {
        let mut controller = ThreadController::new();
        let id = controller.target.id;
        let maps = process::maps::read_maps(id.pid).unwrap();
        let mut sampler = Sampler::new(id);
        sampler.poll(&maps).unwrap();
        for _ in 0..3 {
            let tid = controller.command("start");
            assert_ne!(tid, id.pid);
            sampler.poll(&maps).unwrap();
            let thread = sampler
                .threads
                .get(&tid)
                .expect("new worker was not discovered");
            assert!(thread.event.is_some(), "{:?}", thread.setup_error);
            assert!(thread.switch_event.is_some(), "{:?}", thread.setup_error);
            assert_eq!(controller.command("stop"), tid);
            // pthread_join has completed. Wait only for procfs to drop the task,
            // without racing an uncontrolled stream of replacement threads.
            poll_until(&mut sampler, &maps, |s| !s.threads.contains_key(&tid));
            assert!(!sampler.latest().iter().any(|sample| sample.tid == tid));
            assert!(sampler.threads.contains_key(&id.pid));
        }
        controller.target.assert_detached();
    }

    #[test]
    fn live_sampler_rejects_mismatched_process_identity() {
        let controller = ThreadController::new();
        let id = controller.target.id;
        let maps = process::maps::read_maps(id.pid).unwrap();
        let stale = ProcessId {
            start_time_ticks: id.start_time_ticks + 1,
            ..id
        };
        let mut sampler = Sampler::new(stale);
        assert!(sampler.poll(&maps).is_err());
        assert!(sampler.threads.is_empty());
        controller.target.assert_detached();
    }

    #[test]
    fn live_sampler_rejects_exited_process() {
        let mut controller = ThreadController::new();
        let id = controller.target.id;
        let maps = process::maps::read_maps(id.pid).unwrap();
        let mut sampler = Sampler::new(id);
        sampler.poll(&maps).unwrap();
        controller.target.assert_detached();
        controller.target.child.kill().unwrap();
        controller.target.child.wait().unwrap();
        assert!(sampler.poll(&maps).is_err());
    }
}
