#[cfg(test)]
use super::sample::SampleSource;
use super::sample::monotonic_ns;
use super::{
    disasm::Disassembly,
    perf::Event,
    registers::{self, Register},
    stack::StackFrame,
    symbol::{ElfCache, SymbolInfo, elf_address, instruction_address, resolve_frame},
    unwind::UnwindState,
};
use crate::http_server::process::{self, ProcessId, maps::MemoryMap, procfs};
use anyhow::Result;
use serde::Serialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

const PTRACE_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Clone, Debug, Serialize)]
pub(in crate::http_server::process) struct ThreadSample {
    tid: i32,
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
    fn waiting(tid: i32) -> Self {
        Self {
            tid,
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
    event: Option<Event>,
    switch_event: Option<Event>,
    setup_error: Option<String>,
    ptrace_error: Option<String>,
    latest: ThreadSample,
    sampled_ns: Option<u64>,
}
pub(in crate::http_server::process) struct Sampler {
    id: ProcessId,
    threads: BTreeMap<i32, Thread>,
    symbols: ElfCache,
    unwind: UnwindState,
    next_ptrace: Option<Instant>,
}
impl Sampler {
    pub(in crate::http_server::process) fn new(id: ProcessId) -> Self {
        Self {
            id,
            threads: BTreeMap::new(),
            symbols: ElfCache::default(),
            unwind: UnwindState::default(),
            next_ptrace: None,
        }
    }
    pub(in crate::http_server::process) fn bootstrap(&mut self, maps: &[MemoryMap]) -> Result<()> {
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
        process::check_identity(self.id)
    }
    pub(in crate::http_server::process) fn poll(&mut self, maps: &[MemoryMap]) -> Result<()> {
        process::check_identity(self.id)?;
        self.unwind.refresh(self.id.pid, maps, &mut self.symbols);
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
                let switch_event = match Event::context_switch(tid) {
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
fn process_sample(
    pid: i32,
    unwind: &mut UnwindState,
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
    let (mut frames, reason) = unwind.walk(r.0[8], r.0[7], r.0[6], maps, &sample.stack);
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
    let mut code = Disassembly::capture(pid, r.0[8], maps);
    code.decode();
    let age = monotonic_ns().saturating_sub(sample.time_ns) / 1_000_000;
    thread.sampled_ns = Some(sample.time_ns);
    thread.latest = ThreadSample {
        tid,
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
    fn live_sleeping_bootstrap_and_perf_handoff() {
        let target = TestTarget::new("sleeping");
        let maps = process::maps::read(target.id.pid, false).unwrap();
        let mut sampler = Sampler::new(target.id);
        sampler.bootstrap(&maps).unwrap();
        let first = sampler.latest()[0].clone();
        assert_eq!(first.sample_source, Some(SampleSource::Ptrace), "{first:?}");
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
        if perf_available(target.id.pid) {
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
        }
        drop(sampler);
        target.assert_detached();
    }

    #[test]
    fn periodic_ptrace_waits_for_deadline_and_refreshes_sleeping_thread() {
        let target = TestTarget::new("sleeping");
        let maps = process::maps::read(target.id.pid, false).unwrap();
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
        let maps = process::maps::read(target.id.pid, false).unwrap();
        let mut sampler = Sampler::new(target.id);
        sampler.bootstrap(&maps).unwrap();
        assert!(sampler.threads.len() >= 4);
        let old: BTreeMap<_, _> = sampler
            .threads
            .iter()
            .map(|(&tid, thread)| (tid, thread.sampled_ns))
            .collect();
        // Rediscovery follows the same path as a thread born after bootstrap.
        let rediscovered = *sampler.threads.keys().next().unwrap();
        sampler.threads.remove(&rediscovered);
        sampler.next_ptrace = Some(Instant::now());
        sampler.poll(&maps).unwrap();
        assert!(sampler.threads.contains_key(&rediscovered));
        for (&tid, thread) in &sampler.threads {
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
        let maps = process::maps::read(target.id.pid, false).unwrap();
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
        let maps = process::maps::read(target.id.pid, false).unwrap();
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
        if !perf_available(target.id.pid) {
            return;
        }
        Event::context_switch(target.id.pid).unwrap();
        let maps = process::maps::read(target.id.pid, false).unwrap();
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
        if !perf_available(target.id.pid) {
            return;
        }
        Event::context_switch(target.id.pid).unwrap();
        let maps = process::maps::read(target.id.pid, false).unwrap();
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
        // A main already sleeping can initially have no sample; busy workers continue.
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
        if sleeping.sampled_at != main.sampled_at {
            assert!(matches!(
                sleeping.sample_source,
                Some(SampleSource::ContextSwitch { .. })
            ));
        }
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
