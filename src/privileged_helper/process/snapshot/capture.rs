//! Collect and retain bounded process snapshots from perf and ptrace samples.
//!
//! # Interface
//!
//! | Definition | Visibility | Kind / signature |
//! | --- | --- | --- |
//! | [`Sampler`] | `pub(in crate::privileged_helper::process)` | `struct Sampler` |
//! | [`create_sampler`] | `pub(in crate::privileged_helper::process)` | `fn create_sampler(id: `[`ProcessId`]`) -> `[`Sampler`] |
//! | [`bootstrap_sampler`] | `pub(in crate::privileged_helper::process)` | `fn bootstrap_sampler( state: &mut `[`Sampler`]`, maps: &[`[`MemoryMap`]`], ) -> `[`Result`]`<()>` |
//! | [`poll_sampler`] | `pub(in crate::privileged_helper::process)` | `fn poll_sampler( state: &mut `[`Sampler`]`, maps: &[`[`MemoryMap`]`], ) -> `[`Result`]`<()>` |
//! | [`mapping_snapshots_sampler`] | `pub(in crate::privileged_helper::process)` | `fn mapping_snapshots_sampler( state: &`[`Sampler`]`, ) -> `[`Vec`]`<`[`MappingSnapshot`]`>` |
//! | [`latest_sampler`] | `pub(in crate::privileged_helper::process)` | `fn latest_sampler(state: &`[`Sampler`]`) -> `[`Vec`]`<`[`LiveSample`]`>` |
//!

use super::perf::PerfEventReader;
use super::sample::monotonic_ns;
#[cfg(test)]
use crate::http_server::analysis::snapshot::{
    symbol::{ElfCache, SymbolInfo, elf_address, instruction_address, resolve_frame},
    unwind::StackUnwinder,
};
#[cfg(test)]
use crate::ipc::acquisition::SampleSource;
#[cfg(test)]
use crate::ipc::stack::StackFrame;
use crate::{
    ipc::{
        model::memory::{MappingSnapshot, MemoryMap},
        schema::ProcessId,
    },
    privileged_helper::process::{self, procfs},
};
use anyhow::Result;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::{Duration, Instant},
};

fn executable_header(pid: i32) -> Option<[u8; 20]> {
    use std::io::Read;
    let mut header = [0; 20];
    std::fs::File::open(format!("/proc/{pid}/exe"))
        .and_then(|mut file| file.read_exact(&mut header))
        .ok()?;
    Some(header)
}

const PTRACE_INTERVAL: Duration = Duration::from_secs(10);

use crate::ipc::acquisition::LiveSample;
fn create_waiting_live_sample(tid: i32, executable_header: Option<[u8; 20]>) -> LiveSample {
    LiveSample {
        tid,
        executable_header,
        sampled_at: None,
        sample_time_ns: None,
        #[cfg(test)]
        sample_source: None,
        sample_age_ms: None,
        cpu: None,
        lost_samples: 0,
        registers: Vec::new(),
        raw_registers: None,
        stack_bytes: Vec::new(),
        map_epoch: None,
        context: None,
        call_stack: Vec::new(),
        disassembly: None,
        unwind_stop: "Waiting for sample".into(),
        error: None,
    }
}
struct Thread {
    start_time: u64,
    event: Option<PerfEventReader>,
    switch_event: Option<PerfEventReader>,
    setup_error: Option<String>,
    ptrace_error: Option<String>,
    latest: LiveSample,
    sampled_ns: Option<u64>,
    code_pac_mask: Option<u64>,
}
pub(in crate::privileged_helper::process) struct Sampler {
    id: ProcessId,
    executable_header: Option<[u8; 20]>,
    threads: BTreeMap<i32, Thread>,
    #[cfg(test)]
    symbols: ElfCache,
    #[cfg(test)]
    unwind: StackUnwinder,
    next_ptrace: Option<Instant>,
    context: Arc<MappingSnapshot>,
}
pub(in crate::privileged_helper::process) fn create_sampler(id: ProcessId) -> Sampler {
    Sampler {
        id,
        executable_header: executable_header(id.pid),
        threads: BTreeMap::new(),
        #[cfg(test)]
        symbols: ElfCache::default(),
        #[cfg(test)]
        unwind: StackUnwinder::default(),
        next_ptrace: None,
        context: Arc::new(MappingSnapshot {
            epoch: 0,
            maps: Vec::new(),
        }),
    }
}

/// Captures the initial ptrace snapshot and schedules the next attempt ten seconds
/// after completion, including failed capture attempts.
pub(in crate::privileged_helper::process) fn bootstrap_sampler(
    state: &mut Sampler,
    maps: &[MemoryMap],
) -> Result<()> {
    // Discover all threads and open perf events before interrupting any thread.
    state.next_ptrace = None;
    poll_sampler(state, maps)?;
    capture_ptrace_sampler(state, maps)
}

fn capture_ptrace_sampler(state: &mut Sampler, maps: &[MemoryMap]) -> Result<()> {
    for (&tid, thread) in &mut state.threads {
        thread.ptrace_error = None;
        thread.latest.error = thread.setup_error.clone();
        match super::ptrace::capture_thread_snapshot(tid, maps) {
            Ok(sample) => process_sample(
                state.id.pid,
                #[cfg(test)]
                &mut state.unwind,
                #[cfg(test)]
                &mut state.symbols,
                thread,
                sample,
                maps,
                Arc::clone(&state.context),
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
    state.next_ptrace = Some(Instant::now() + PTRACE_INTERVAL);
    process::validate_process_identity(state.id)
}

/// Drains perf samples and captures current threads when the ptrace deadline is due.
pub(in crate::privileged_helper::process) fn poll_sampler(
    state: &mut Sampler,
    maps: &[MemoryMap],
) -> Result<()> {
    #[cfg(test)]
    use crate::http_server::analysis::snapshot::symbol::cache_elf_symbols_from_bytes;
    #[cfg(test)]
    use crate::http_server::analysis::snapshot::symbol::lookup_elf_symbols;
    #[cfg(test)]
    use crate::http_server::analysis::snapshot::unwind::refresh_stack_unwinder;
    use {
        crate::privileged_helper::process::snapshot::perf::context_switch_perf_event_reader,
        crate::privileged_helper::process::snapshot::perf::drain_perf_event_reader,
        crate::privileged_helper::process::snapshot::perf::open_perf_event_reader,
    };

    process::validate_process_identity(state.id)?;
    if state.context.maps.as_slice() != maps {
        state.context = Arc::new(MappingSnapshot {
            epoch: state.context.epoch + 1,
            maps: maps.to_vec(),
        });
    }
    #[cfg(test)]
    {
        for map in maps.iter().filter(|map| map.executable && map.inode != 0) {
            if lookup_elf_symbols(&state.symbols, map).is_none()
                && let Some(file) = process::elf::read_mapped_elf_file(state.id.pid, map)
            {
                cache_elf_symbols_from_bytes(&mut state.symbols, map, file);
            }
        }
        refresh_stack_unwinder(&mut state.unwind, maps, &state.symbols);
    }
    let mut alive = BTreeSet::new();
    for tid in super::super::threads::collect_thread_ids(state.id.pid)? {
        let Ok(stat) =
            procfs::read_process_stat(&format!("/proc/{}/task/{tid}/stat", state.id.pid))
        else {
            continue;
        };
        alive.insert(tid);
        if state
            .threads
            .get(&tid)
            .is_some_and(|thread| thread.start_time != stat.start_time)
        {
            state.threads.remove(&tid);
        }
        let thread = state.threads.entry(tid).or_insert_with(|| {
            let mut latest = create_waiting_live_sample(tid, state.executable_header);
            let event = match open_perf_event_reader(tid) {
                Ok(event) => Some(event),
                Err(e) => {
                    latest.error = Some(format!("{e:#}"));
                    None
                }
            };
            let switch_event = match context_switch_perf_event_reader(tid) {
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
                code_pac_mask: None,
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
            match drain_perf_event_reader(&mut *event) {
                Ok(Some(sample)) if sample.tid == tid => {
                    if newest
                        .as_ref()
                        .is_none_or(|old: &super::sample::RawSample| sample.time_ns > old.time_ns)
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
                state.id.pid,
                #[cfg(test)]
                &mut state.unwind,
                #[cfg(test)]
                &mut state.symbols,
                thread,
                sample,
                maps,
                Arc::clone(&state.context),
            );
        }
    }
    state.threads.retain(|tid, _| alive.contains(tid));
    if state
        .next_ptrace
        .is_some_and(|deadline| Instant::now() >= deadline)
    {
        capture_ptrace_sampler(state, maps)?;
    }
    process::validate_process_identity(state.id)?;
    Ok(())
}

/// Returns each mapping context referenced by retained thread samples once.
pub(in crate::privileged_helper::process) fn mapping_snapshots_sampler(
    state: &Sampler,
) -> Vec<MappingSnapshot> {
    let mut snapshots = BTreeMap::new();
    for thread in state.threads.values() {
        if let Some(context) = &thread.latest.context {
            snapshots
                .entry(context.epoch)
                .or_insert_with(|| context.as_ref().clone());
        }
    }
    snapshots.into_values().collect()
}

/// Returns the latest samples with ages refreshed from monotonic time.
pub(in crate::privileged_helper::process) fn latest_sampler(state: &Sampler) -> Vec<LiveSample> {
    let now = monotonic_ns();
    state
        .threads
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

fn process_sample(
    pid: i32,
    #[cfg(test)] unwind: &mut StackUnwinder,
    #[cfg(test)] symbols: &mut ElfCache,
    thread: &mut Thread,
    mut sample: super::sample::RawSample,
    maps: &[MemoryMap],
    context: Arc<MappingSnapshot>,
) {
    #[cfg(test)]
    use crate::http_server::analysis::snapshot::symbol::lookup_elf_symbols;
    #[cfg(test)]
    use crate::http_server::analysis::snapshot::unwind::unwind_stack;

    #[cfg(test)]
    use crate::ipc::model::memory::memory_map_contains;
    use {
        crate::ipc::registers::pc_register_set,
        crate::privileged_helper::process::snapshot::instruction_bytes::capture_instruction_bytes,
    };

    if thread.sampled_ns.is_some_and(|time| sample.time_ns <= time) {
        return;
    }
    let executable_header = thread.latest.executable_header;
    let tid = sample.tid;
    let lost = thread.latest.lost_samples;
    thread.code_pac_mask = sample.registers.code_pac_mask.or(thread.code_pac_mask);
    sample.registers.code_pac_mask = thread.code_pac_mask;
    let r = &sample.registers;
    #[cfg(test)]
    let (frames, reason) = {
        let (mut frames, reason) = unwind_stack(&mut *unwind, r, maps, &sample.stack);
        for (index, frame) in frames.iter_mut().enumerate() {
            let address = instruction_address(frame.address, index > 0);
            let info = maps
                .iter()
                .find(|m| memory_map_contains(m, address) && m.inode != 0)
                .and_then(|map| {
                    let elf = lookup_elf_symbols(symbols, map)?;
                    resolve_frame(elf_address(address, map, &elf, procfs::page_size())?, &elf)
                });
            apply_symbol_info(frame, info);
        }
        (frames, reason)
    };
    #[cfg(not(test))]
    let (frames, reason) = (Vec::new(), String::new());
    let code = capture_instruction_bytes(pid, pc_register_set(r), maps);
    let age = monotonic_ns().saturating_sub(sample.time_ns) / 1_000_000;
    thread.sampled_ns = Some(sample.time_ns);
    thread.latest = LiveSample {
        tid,
        executable_header,
        sampled_at: Some(crate::privileged_helper::timestamp_ms().saturating_sub(age)),
        sample_time_ns: Some(sample.time_ns),
        #[cfg(test)]
        sample_source: Some(sample.source),
        sample_age_ms: Some(age),
        cpu: sample.cpu,
        lost_samples: lost,
        #[cfg(test)]
        registers: crate::http_server::analysis::snapshot::registers::classify_sample_registers(
            r, maps,
        ),
        #[cfg(not(test))]
        registers: Vec::new(),
        raw_registers: Some(r.clone()),
        stack_bytes: sample.stack.clone(),
        map_epoch: Some(context.epoch),
        context: Some(context),
        call_stack: frames,
        disassembly: Some(code),
        unwind_stop: reason,
        error: thread.latest.error.clone(),
    };
}

// Replace all resolved fields so repeated application cannot append inline frames
// or preserve stale data from an earlier resolution.
#[cfg(test)]
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
    use {super::*, crate::ipc::stack::SourceFrame};

    #[test]
    fn applying_symbols_replaces_fields_and_preserves_json_contract() {
        use crate::http_server::analysis::snapshot::stack::create_raw_stack_frame;

        let mut frame = create_raw_stack_frame(0x1234);
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
        assert_eq!(frame, create_raw_stack_frame(0x1234));
    }
}

#[cfg(test)]
mod live_tests {
    use {
        super::{Sampler, *},
        crate::privileged_helper::process::Target,
        std::{
            io::{BufRead, BufReader, Write},
            os::{fd::OwnedFd, unix::net::UnixStream},
            process::{Command, Stdio},
            time::{Duration, Instant},
        },
    };

    /// Commands one worker at a time and waits for readiness or completed join.
    struct ThreadController {
        target: Target,
        connection: BufReader<UnixStream>,
    }

    fn create_thread_controller() -> ThreadController {
        crate::test_support::build_targets();
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
        let id = process::read_process_identity(child.id() as i32).unwrap();
        let mut controller = ThreadController {
            target: Target {
                child,
                id,
                address: 0,
            },
            connection: BufReader::new(connection),
        };
        assert_eq!(response_thread_controller(&mut controller).trim(), "ready");
        controller
    }

    fn response_thread_controller(state: &mut ThreadController) -> String {
        let mut response = String::new();
        assert!(
            state.connection.read_line(&mut response).unwrap() > 0,
            "lifecycle fixture closed its control connection"
        );
        response
    }

    fn command_thread_controller(state: &mut ThreadController, command: &str) -> i32 {
        writeln!(state.connection.get_mut(), "{command}").unwrap();
        response_thread_controller(state).trim().parse().unwrap()
    }

    fn poll_until(sampler: &mut Sampler, maps: &[MemoryMap], condition: impl Fn(&Sampler) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            super::poll_sampler(&mut *sampler, maps).unwrap();
            if condition(sampler) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "no expected live sample: {:?}",
            super::latest_sampler(sampler)
        );
    }

    #[test]
    fn live_sleeping_bootstrap_and_perf_handoff() {
        use crate::privileged_helper::process::{
            create_target, snapshot::perf::open_perf_event_reader,
            test_support::assert_detached_target,
        };

        let target = create_target("sleeping");
        let maps = process::maps::read_maps(target.id.pid).unwrap();
        let mut sampler = super::create_sampler(target.id);
        super::bootstrap_sampler(&mut sampler, &maps).unwrap();
        let first = super::latest_sampler(&sampler)[0].clone();
        assert_eq!(first.sample_source, Some(SampleSource::Ptrace), "{first:?}");
        assert_eq!(
            serde_json::to_value(&first).unwrap()["architecture"],
            std::env::consts::ARCH
        );
        assert_eq!(
            first.registers.len(),
            if cfg!(target_arch = "aarch64") {
                if first.raw_registers.as_ref().unwrap().flags_available {
                    34
                } else {
                    33
                }
            } else {
                18
            }
        );
        assert!(first.cpu.is_none());
        assert!(
            first
                .call_stack
                .iter()
                .any(|frame| frame.symbol.as_deref() == Some("main")),
            "{:?}",
            first.call_stack
        );
        assert_detached_target(&target);
        open_perf_event_reader(target.id.pid)
            .expect("perf access is required in the Docker test container");
        poll_until(&mut sampler, &maps, |s| {
            super::latest_sampler(s)[0].sampled_at > first.sampled_at
                && super::latest_sampler(s)[0].sample_source != Some(SampleSource::Ptrace)
        });
        assert_detached_target(&target);
        // The periodic capture also hands control back to real perf updates.
        sampler.next_ptrace = Some(Instant::now());
        super::poll_sampler(&mut sampler, &maps).unwrap();
        assert_eq!(
            super::latest_sampler(&sampler)[0].sample_source,
            Some(SampleSource::Ptrace)
        );
        let periodic_ns = sampler.threads[&target.id.pid].sampled_ns;
        poll_until(&mut sampler, &maps, |s| {
            s.threads[&target.id.pid].sampled_ns > periodic_ns
                && super::latest_sampler(s)[0].sample_source != Some(SampleSource::Ptrace)
        });
        assert_detached_target(&target);
        drop(sampler);
        assert_detached_target(&target);
    }

    #[test]
    fn periodic_ptrace_waits_for_deadline_and_refreshes_sleeping_thread() {
        use crate::privileged_helper::process::{
            create_target, test_support::assert_detached_target,
        };

        let target = create_target("sleeping");
        let maps = process::maps::read_maps(target.id.pid).unwrap();
        let mut sampler = super::create_sampler(target.id);
        super::bootstrap_sampler(&mut sampler, &maps).unwrap();
        let deadline = sampler.next_ptrace.unwrap();
        assert!(deadline.duration_since(Instant::now()) > Duration::from_secs(9));
        // Isolate the periodic source from interrupt-induced context-switch samples.
        let thread = sampler.threads.get_mut(&target.id.pid).unwrap();
        thread.event = None;
        thread.switch_event = None;
        let first = thread.sampled_ns;
        super::poll_sampler(&mut sampler, &maps).unwrap();
        assert_eq!(sampler.threads[&target.id.pid].sampled_ns, first);
        assert_eq!(sampler.next_ptrace, Some(deadline));
        sampler.next_ptrace = Some(Instant::now() - Duration::from_secs(30));
        super::poll_sampler(&mut sampler, &maps).unwrap();
        let thread = &sampler.threads[&target.id.pid];
        assert!(thread.sampled_ns > first);
        assert_eq!(thread.latest.sample_source, Some(SampleSource::Ptrace));
        assert!(thread.ptrace_error.is_none());
        assert_eq!(thread.latest.error, thread.setup_error);
        assert!(
            sampler.next_ptrace.unwrap().duration_since(Instant::now()) > Duration::from_secs(9)
        );
        let second = thread.sampled_ns;
        super::poll_sampler(&mut sampler, &maps).unwrap();
        assert_eq!(sampler.threads[&target.id.pid].sampled_ns, second);
        assert_detached_target(&target);
    }

    #[test]
    fn periodic_ptrace_captures_all_threads_including_newly_discovered_threads() {
        use crate::privileged_helper::process::{
            create_target, test_support::assert_detached_target,
        };

        let target = create_target("threads");
        let maps = process::maps::read_maps(target.id.pid).unwrap();
        let mut sampler = super::create_sampler(target.id);
        super::bootstrap_sampler(&mut sampler, &maps).unwrap();
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
        super::poll_sampler(&mut sampler, &maps).unwrap();
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
        assert_detached_target(&target);
    }

    #[test]
    fn periodic_ptrace_failure_preserves_sample_and_retries_only_when_due() {
        use crate::privileged_helper::process::{
            create_target, test_support::assert_detached_target,
        };

        let target = create_target("sleeping");
        let maps = process::maps::read_maps(target.id.pid).unwrap();
        let mut sampler = super::create_sampler(target.id);
        super::bootstrap_sampler(&mut sampler, &maps).unwrap();
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
        super::poll_sampler(&mut sampler, &maps).unwrap();
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
        assert_detached_target(&target);
        super::poll_sampler(&mut sampler, &maps).unwrap();
        assert_eq!(sampler.next_ptrace, Some(failed_deadline));
        assert_eq!(sampler.threads[&target.id.pid].sampled_ns, first);
        assert!(sampler.threads[&target.id.pid].latest.error.is_some());
        sampler.next_ptrace = Some(Instant::now());
        super::poll_sampler(&mut sampler, &maps).unwrap();
        let thread = &sampler.threads[&target.id.pid];
        assert!(thread.sampled_ns > first);
        assert!(thread.ptrace_error.is_none());
        assert_eq!(thread.latest.error, thread.setup_error);
        assert_eq!(thread.latest.sample_source, Some(SampleSource::Ptrace));
        assert_detached_target(&target);
    }

    #[test]
    fn sample_ordering_preserves_bootstrap_then_accepts_newer_perf() {
        use crate::privileged_helper::process::{
            create_target, test_support::assert_detached_target,
        };

        let target = create_target("sleeping");
        let maps = process::maps::read_maps(target.id.pid).unwrap();
        let mut sampler = super::create_sampler(target.id);
        super::bootstrap_sampler(&mut sampler, &maps).unwrap();
        let thread = sampler.threads.get_mut(&target.id.pid).unwrap();
        let time = thread.sampled_ns.unwrap();
        let sample = super::super::sample::RawSample {
            tid: target.id.pid,
            time_ns: time.saturating_sub(1),
            cpu: Some(0),
            registers: crate::ipc::registers::RegisterSet::default(),
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
            Arc::clone(&sampler.context),
        );
        assert_eq!(thread.sampled_ns, Some(time));
        assert_eq!(thread.latest.sample_source, Some(SampleSource::Ptrace));
        let sample = super::super::sample::RawSample {
            tid: target.id.pid,
            time_ns: time + 1,
            cpu: Some(7),
            registers: crate::ipc::registers::RegisterSet::default(),
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
            Arc::clone(&sampler.context),
        );
        assert_eq!(thread.sampled_ns, Some(time + 1));
        assert_eq!(thread.latest.sample_source, Some(SampleSource::CpuClock));
        assert_eq!(thread.latest.cpu, Some(7));
        assert_detached_target(&target);
    }

    #[test]
    fn live_blocked_samples_refresh_known_registers_and_bound_deep_stacks() {
        use crate::privileged_helper::process::{
            create_target,
            snapshot::perf::{context_switch_perf_event_reader, open_perf_event_reader},
            test_support::assert_detached_target,
        };

        let target = create_target("blocked_stack");
        open_perf_event_reader(target.id.pid)
            .expect("perf access is required in the Docker test container");
        context_switch_perf_event_reader(target.id.pid).unwrap();
        let maps = process::maps::read_maps(target.id.pid).unwrap();
        let mut sampler = super::create_sampler(target.id);
        let mut complete = false;
        let mut truncated = false;
        let mut previous = None;
        let mut updates = 0;
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline {
            super::poll_sampler(&mut sampler, &maps).unwrap();
            let sample = &super::latest_sampler(&sampler)[0];
            if sample.sample_source == Some(SampleSource::ContextSwitch { preempted: false }) {
                assert!(sample.error.is_none(), "{:?}", sample.error);
                let names = if cfg!(target_arch = "aarch64") {
                    ["X19", "X20", "X21", "X22"]
                } else {
                    ["R12", "R13", "R14", "R15"]
                };
                for (name, expected) in [
                    (names[0], 0x12345678u64),
                    (names[1], 0x23456789),
                    (names[2], 0x3456789a),
                    (names[3], 0x456789ab),
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
            super::latest_sampler(&sampler)
        );
        eprintln!(
            "Blocked fixture: {updates} distinct updates in 3s; R12–R15 matched; complete and bounded deep stacks verified"
        );
        assert_detached_target(&target);
    }

    #[test]
    fn live_busy_and_recursive_samples_advance_without_tracing() {
        use crate::privileged_helper::process::{
            create_target, snapshot::perf::open_perf_event_reader,
            test_support::assert_detached_target,
        };

        for name in [
            "busy_loop",
            "recursive",
            "recursive_no_fp",
            "recursive_no_fp_nopie",
        ] {
            let target = create_target(name);
            open_perf_event_reader(target.id.pid)
                .expect("perf access is required in the Docker test container");
            let maps = process::maps::read_maps(target.id.pid).unwrap();
            let mut sampler = super::create_sampler(target.id);
            poll_until(&mut sampler, &maps, |s| {
                super::latest_sampler(s)
                    .iter()
                    .any(|t| t.sampled_at.is_some())
            });
            let first = super::latest_sampler(&sampler)[0].clone();
            assert_eq!(
                first.registers.len(),
                if cfg!(target_arch = "aarch64") {
                    if first.raw_registers.as_ref().unwrap().flags_available {
                        34
                    } else {
                        33
                    }
                } else {
                    18
                }
            );
            assert!(!first.disassembly.as_ref().unwrap().bytes.is_empty());
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
                super::latest_sampler(s)[0].sampled_at > first.sampled_at
            });
            assert_detached_target(&target);
            drop(sampler);
            assert_detached_target(&target);
        }
    }

    #[test]
    fn live_no_fp_unwinds_across_shared_library_and_libc() {
        use crate::{
            ipc::model::memory::memory_map_contains,
            privileged_helper::process::{
                create_target,
                snapshot::perf::{context_switch_perf_event_reader, open_perf_event_reader},
                test_support::assert_detached_target,
            },
        };

        let target = create_target("shared_no_fp");
        open_perf_event_reader(target.id.pid)
            .expect("perf access is required in the Docker test container");
        context_switch_perf_event_reader(target.id.pid).unwrap();
        let maps = process::maps::read_maps(target.id.pid).unwrap();
        let mut sampler = super::create_sampler(target.id);
        poll_until(&mut sampler, &maps, |s| {
            let latest = super::latest_sampler(s);
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
                            memory_map_contains(map, f.address)
                                && map
                                    .pathname
                                    .as_deref()
                                    .is_some_and(|p| p.contains("libc.so"))
                        })
                    })
            })
        });
        assert_detached_target(&target);
    }

    #[test]
    fn live_thread_lifecycle_discovers_and_removes_acknowledged_threads() {
        use crate::privileged_helper::process::test_support::assert_detached_target;

        let mut controller = create_thread_controller();
        let id = controller.target.id;
        let maps = process::maps::read_maps(id.pid).unwrap();
        let mut sampler = super::create_sampler(id);
        super::poll_sampler(&mut sampler, &maps).unwrap();
        for _ in 0..3 {
            let tid = command_thread_controller(&mut controller, "start");
            assert_ne!(tid, id.pid);
            super::poll_sampler(&mut sampler, &maps).unwrap();
            let thread = sampler
                .threads
                .get(&tid)
                .expect("new worker was not discovered");
            assert!(thread.event.is_some(), "{:?}", thread.setup_error);
            assert!(thread.switch_event.is_some(), "{:?}", thread.setup_error);
            assert_eq!(command_thread_controller(&mut controller, "stop"), tid);
            // pthread_join has completed. Wait only for procfs to drop the task,
            // without racing an uncontrolled stream of replacement threads.
            poll_until(&mut sampler, &maps, |s| !s.threads.contains_key(&tid));
            assert!(
                !super::latest_sampler(&sampler)
                    .iter()
                    .any(|sample| sample.tid == tid)
            );
            assert!(sampler.threads.contains_key(&id.pid));
        }
        assert_detached_target(&controller.target);
    }

    #[test]
    fn live_sampler_rejects_mismatched_process_identity() {
        use crate::privileged_helper::process::test_support::assert_detached_target;

        let controller = create_thread_controller();
        let id = controller.target.id;
        let maps = process::maps::read_maps(id.pid).unwrap();
        let stale = ProcessId {
            start_time_ticks: id.start_time_ticks + 1,
            ..id
        };
        let mut sampler = super::create_sampler(stale);
        assert!(super::poll_sampler(&mut sampler, &maps).is_err());
        assert!(sampler.threads.is_empty());
        assert_detached_target(&controller.target);
    }

    #[test]
    fn live_sampler_rejects_exited_process() {
        use crate::privileged_helper::process::test_support::assert_detached_target;

        let mut controller = create_thread_controller();
        let id = controller.target.id;
        let maps = process::maps::read_maps(id.pid).unwrap();
        let mut sampler = super::create_sampler(id);
        super::poll_sampler(&mut sampler, &maps).unwrap();
        assert_detached_target(&controller.target);
        controller.target.child.kill().unwrap();
        controller.target.child.wait().unwrap();
        assert!(super::poll_sampler(&mut sampler, &maps).is_err());
    }
}
