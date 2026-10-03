//! Opt-in syscall experiment. Production sampling never enables these events.
use super::{
    perf::{Event, REGS_MASK, Sample},
    registers::RegisterSet,
};
use anyhow::{Result, ensure};

struct RawSample<'a> {
    tid: i32,
    time_ns: u64,
    cpu: u32,
    data: &'a [u8],
    rest: &'a [u8],
}
fn raw(bytes: &[u8]) -> Result<RawSample<'_>> {
    ensure!(bytes.len() >= 28, "truncated tracepoint sample");
    let tid = i32::from_ne_bytes(bytes[4..8].try_into()?);
    let time = u64::from_ne_bytes(bytes[8..16].try_into()?);
    let cpu = u32::from_ne_bytes(bytes[16..20].try_into()?);
    let size = u32::from_ne_bytes(bytes[24..28].try_into()?) as usize;
    let end = 28usize
        .checked_add(size)
        .ok_or_else(|| anyhow::anyhow!("raw size overflow"))?;
    ensure!(end <= bytes.len(), "truncated RAW");
    Ok(RawSample {
        tid,
        time_ns: time,
        cpu,
        data: &bytes[28..end],
        rest: &bytes[end..],
    })
}

#[test]
#[ignore = "requires dev_run.sh / CAP_PERFMON; only observes its own getpid syscall"]
fn syscall_tracepoint_ids() -> Result<()> {
    let tid = unsafe { libc::syscall(libc::SYS_gettid) } as i32;
    let pid = std::process::id() as i64;
    drop(Event::open(tid)?); // Fail clearly on missing perf permissions before probing IDs.
    let mut enter = None;
    let mut exit = None;
    for id in 1..=65535 {
        let Ok(mut event) = Event::tracepoint(tid, id, None, Some("id == 39")) else {
            continue;
        };
        unsafe {
            libc::syscall(libc::SYS_getpid);
        }
        event.records(|kind, _, bytes| {
            if kind == 9 {
                let RawSample { data, .. } = raw(bytes)?;
                if data.len() >= 24 && i64::from_ne_bytes(data[8..16].try_into()?) == 39 {
                    if (64..72).contains(&data.len()) {
                        enter = Some(id);
                    }
                    if (24..32).contains(&data.len())
                        && i64::from_ne_bytes(data[16..24].try_into()?) == pid
                    {
                        exit = Some(id);
                    }
                }
            }
            Ok(())
        })?;
        if enter.is_some() && exit.is_some() {
            break;
        }
    }
    ensure!(
        enter.is_some() && exit.is_some(),
        "raw syscall tracepoints not found"
    );
    eprintln!(
        "SYSCALL_TRACEPOINT_IDS enter={} exit={}",
        enter.unwrap(),
        exit.unwrap()
    );
    Ok(())
}

fn u64_from(bytes: &mut &[u8]) -> Result<u64> {
    ensure!(bytes.len() >= 8, "truncated register / stack sample");
    let value = u64::from_ne_bytes(bytes[..8].try_into()?);
    *bytes = &bytes[8..];
    Ok(value)
}

fn user_sample(tid: i32, time_ns: u64, cpu: u32, mut bytes: &[u8]) -> Result<Option<Sample>> {
    let abi = u64_from(&mut bytes)?;
    if abi == 0 {
        return Ok(None);
    }
    ensure!(abi == 2, "not x86-64 user ABI");
    let mut registers = [0; 24];
    for (index, value) in registers.iter_mut().enumerate() {
        if REGS_MASK & (1 << index) != 0 {
            *value = u64_from(&mut bytes)?;
        }
    }
    let size = usize::try_from(u64_from(&mut bytes)?)?;
    ensure!(size <= bytes.len(), "truncated user stack");
    let (stack, mut rest) = bytes.split_at(size);
    let used = if size == 0 {
        0
    } else {
        usize::try_from(u64_from(&mut rest)?)?
    };
    ensure!(used <= size, "invalid dynamic stack size");
    Ok(Some(Sample {
        tid,
        time_ns,
        cpu,
        registers: RegisterSet(registers),
        stack: stack[..used].to_vec(),
    }))
}

struct Call {
    tid: i32,
    time_ns: u64,
    id: i64,
    args: Option<[u64; 6]>,
    ret: Option<i64>,
    user: Option<Sample>,
}
fn call(bytes: &[u8], enter: bool, full: bool, tracepoint: u64) -> Result<Call> {
    let RawSample {
        tid,
        time_ns,
        cpu,
        data,
        rest,
    } = raw(bytes)?;
    ensure!(
        data.len() >= if enter { 64 } else { 24 },
        "truncated syscall RAW"
    );
    ensure!(
        u16::from_ne_bytes(data[..2].try_into()?) as u64 == tracepoint,
        "wrong tracepoint common_type"
    );
    ensure!(
        i32::from_ne_bytes(data[4..8].try_into()?) == tid,
        "wrong tracepoint common_pid"
    );
    let id = i64::from_ne_bytes(data[8..16].try_into()?);
    let args = if enter {
        let mut args = [0; 6];
        for (index, arg) in args.iter_mut().enumerate() {
            *arg = u64::from_ne_bytes(data[16 + index * 8..24 + index * 8].try_into()?);
        }
        Some(args)
    } else {
        None
    };
    Ok(Call {
        tid,
        time_ns,
        id,
        args,
        ret: if enter {
            None
        } else {
            Some(i64::from_ne_bytes(data[16..24].try_into()?))
        },
        user: if full {
            user_sample(tid, time_ns, cpu, rest)?
        } else {
            None
        },
    })
}

#[derive(Default)]
struct DurationStats {
    count: u64,
    sum: u128,
    min: Option<u64>,
    max: u64,
    last_ret: i64,
}
#[derive(Default)]
struct Tracker {
    pending: std::collections::BTreeMap<i32, Call>,
    durations: std::collections::BTreeMap<i64, DurationStats>,
    unmatched_exit: u64,
    overwritten_enter: u64,
    order_errors: u64,
    invalidations: u64,
    last_time: Option<u64>,
    latest_user: Option<(i64, Sample)>,
}
impl Tracker {
    fn ingest(&mut self, event: Call) {
        if self.last_time.is_some_and(|time| event.time_ns < time) {
            self.order_errors += 1;
            self.pending.clear();
            self.latest_user = None;
            return;
        }
        self.last_time = Some(event.time_ns);
        let Some(ret) = event.ret else {
            if self.pending.insert(event.tid, event).is_some() {
                self.overwritten_enter += 1;
            }
            return;
        };
        if let Some(enter) = self.pending.remove(&event.tid) {
            if enter.id != event.id || enter.time_ns > event.time_ns {
                self.unmatched_exit += 1;
                return;
            }
            let duration = event.time_ns - enter.time_ns;
            let stats = self.durations.entry(event.id).or_default();
            stats.count += 1;
            stats.sum += duration as u128;
            stats.min = Some(stats.min.map_or(duration, |min| min.min(duration)));
            stats.max = stats.max.max(duration);
            stats.last_ret = ret;
            if let Some(sample) = enter.user {
                self.latest_user = Some((enter.id, sample));
            }
        } else {
            self.unmatched_exit += 1;
        }
    }
    fn invalidate(&mut self) {
        self.pending.clear();
        self.latest_user = None;
        self.invalidations += 1;
    }
}
fn syscall_name(id: i64) -> String {
    // Linux x86-64 names used by the workloads and blocking-candidate filter.
    let names = [
        (0, "read"),
        (1, "write"),
        (3, "close"),
        (7, "poll"),
        (23, "select"),
        (35, "nanosleep"),
        (39, "getpid"),
        (42, "connect"),
        (43, "accept"),
        (45, "recvfrom"),
        (47, "recvmsg"),
        (61, "wait4"),
        (74, "fsync"),
        (202, "futex"),
        (230, "clock_nanosleep"),
        (232, "epoll_wait"),
        (247, "waitid"),
        (270, "pselect6"),
        (271, "ppoll"),
        (281, "epoll_pwait"),
        (288, "accept4"),
        (299, "recvmmsg"),
        (426, "io_uring_enter"),
        (441, "epoll_pwait2"),
    ];
    names
        .iter()
        .find(|(number, _)| *number == id)
        .map_or_else(|| format!("syscall_{id}"), |(_, name)| (*name).into())
}
const BLOCKING_FILTER: &str = "id == 0 || id == 7 || id == 23 || id == 35 || id == 42 || id == 43 || id == 45 || id == 47 || id == 61 || id == 74 || id == 202 || id == 230 || id == 232 || id == 247 || id == 270 || id == 271 || id == 281 || id == 288 || id == 299 || id == 426 || id == 441";

#[test]
#[ignore = "requires target TID and tracepoint IDs; see docs/SYSCALL_TRACEPOINT_POC.md"]
fn syscall_tracepoint_poc() -> Result<()> {
    let tid: i32 = std::env::var("PROCINSH_SYSCALL_TID")?.parse()?;
    ensure!(tid > 0, "TID must be positive");
    let enter_id: u64 = std::env::var("PROCINSH_SYS_ENTER_ID")?.parse()?;
    let exit_id: u64 = std::env::var("PROCINSH_SYS_EXIT_ID")?.parse()?;
    let size: u32 = std::env::var("PROCINSH_SYSCALL_STACK")
        .unwrap_or("2048".into())
        .parse()?;
    ensure!(
        [0, 2048, 4096, 8192].contains(&size),
        "stack must be 0, 2048, 4096 or 8192"
    );
    let full = size != 0;
    let full_exit = std::env::var_os("PROCINSH_SYSCALL_EXIT_STACK").is_some() && full;
    let seconds: u64 = std::env::var("PROCINSH_SYSCALL_SECONDS")
        .unwrap_or("5".into())
        .parse()?;
    let filter = std::env::var_os("PROCINSH_SYSCALL_FILTER").map(|_| BLOCKING_FILTER);
    // Exit first: attachment to an already-running syscall can yield an unmatched exit.
    let mut exits = Event::tracepoint(
        tid,
        exit_id,
        if full_exit { Some(size) } else { None },
        filter,
    )?;
    let mut enters =
        Event::tracepoint(tid, enter_id, if full { Some(size) } else { None }, filter)?;
    let mut context = if std::env::var_os("PROCINSH_SYSCALL_CONTEXT").is_some() {
        Some(Event::context_switch(tid, 2048)?)
    } else {
        None
    };
    let mappings = crate::http_server::process::maps::read(tid, false)?;
    let mut tracker = Tracker::default();
    let mut queue = Vec::new();
    let mut counts = [0u64; 2];
    let mut abi_none = 0u64;
    let mut ring_bytes = 0u64;
    let mut context_counts = [0u64; 3];
    let mut last_lost = 0u64;
    let mut context_lost = 0u64;
    let mut throttles = [0u64; 2];
    let start = std::time::Instant::now();
    let cpu_start = clock_ns(libc::CLOCK_PROCESS_CPUTIME_ID);
    let mut next_pending = 1u64;
    while start.elapsed() < std::time::Duration::from_secs(seconds) {
        // Records newer than this cutoff stay queued until the next poll so an exit
        // cannot overtake an enter from the other ring during the two drains.
        let cutoff = clock_ns(libc::CLOCK_MONOTONIC);
        let mut discontinuity = false;
        for (index, event, capture, id) in [
            (0, &mut enters, full, enter_id),
            (1, &mut exits, full_exit, exit_id),
        ] {
            event.records(|kind, _, bytes| {
                ring_bytes += bytes.len() as u64 + 8;
                if kind == 5 || kind == 6 {
                    throttles[index] += 1;
                    discontinuity = true;
                }
                if kind == 9 {
                    counts[index] += 1;
                    let event = call(bytes, index == 0, capture, id)?;
                    ensure!(event.tid == tid, "unexpected TID");
                    if capture && event.user.is_none() {
                        abi_none += 1;
                    }
                    queue.push(event);
                }
                Ok(())
            })?;
        }
        let lost = enters.lost.saturating_add(exits.lost);
        if lost != last_lost || discontinuity {
            tracker.invalidate();
            queue.clear();
            last_lost = lost;
        } else {
            queue.sort_by_key(|event| event.time_ns);
            let count = queue.partition_point(|event| event.time_ns <= cutoff);
            for event in queue.drain(..count) {
                tracker.ingest(event);
            }
        }
        if let Some(context) = &mut context {
            context.records(|kind, misc, _| {
                if kind == 9 {
                    context_counts[0] += 1;
                }
                if kind == 14 && misc & (1 << 13) != 0 {
                    context_counts[if misc & (1 << 14) == 0 { 1 } else { 2 }] += 1;
                }
                Ok(())
            })?;
            context_lost = context.lost;
        }
        if start.elapsed().as_secs() >= next_pending {
            for enter in tracker.pending.values() {
                let age = cutoff.saturating_sub(enter.time_ns);
                if age >= 500_000_000 {
                    eprintln!(
                        "IN_SYSCALL tid={} syscall={} id={} age_ms={} args={:?}",
                        enter.tid,
                        syscall_name(enter.id),
                        enter.id,
                        age / 1_000_000,
                        enter.args
                    );
                }
            }
            next_pending += 1;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    let cpu = clock_ns(libc::CLOCK_PROCESS_CPUTIME_ID).saturating_sub(cpu_start);
    let mut summary = serde_json::json!({"tid":tid,"enter_id":enter_id,"exit_id":exit_id,"stack_size":size,"exit_stack":full_exit,"filtered":filter.is_some(),"elapsed_s":start.elapsed().as_secs_f64(),"observer_cpu_ns":cpu,"enter_samples":counts[0],"exit_samples":counts[1],"abi_none":abi_none,"ring_bytes":ring_bytes,"enter_lost":enters.lost,"exit_lost":exits.lost,"unmatched_exit":tracker.unmatched_exit,"overwritten_enter":tracker.overwritten_enter,"order_errors":tracker.order_errors,"invalidations":tracker.invalidations,"queued_after_cutoff":queue.len(),"enter_throttle_records":throttles[0],"exit_throttle_records":throttles[1],"context_samples":context_counts[0],"voluntary_out":context_counts[1],"preempt_out":context_counts[2],"context_lost":context_lost});
    let durations: Vec<_> = tracker.durations.iter().map(|(id,s)| serde_json::json!({"id":id,"syscall":syscall_name(*id),"count":s.count,"min_ns":s.min,"mean_ns":(s.sum / s.count as u128) as u64,"max_ns":s.max,"last_ret":s.last_ret})).collect();
    summary["durations"] = serde_json::json!(durations);
    summary["pending"] = serde_json::json!(tracker.pending.values().map(|call| serde_json::json!({"tid":call.tid,"id":call.id,"syscall":syscall_name(call.id),"args":call.args,"age_ms":clock_ns(libc::CLOCK_MONOTONIC).saturating_sub(call.time_ns)/1_000_000})).collect::<Vec<_>>());
    let status = std::fs::read_to_string(format!("/proc/{tid}/status"))?;
    summary["target_status"] = serde_json::json!(
        status
            .lines()
            .filter(|line| line.starts_with("State:") || line.starts_with("TracerPid:"))
            .collect::<Vec<_>>()
    );
    let selected = tracker
        .pending
        .values()
        .find_map(|call| {
            call.user
                .as_ref()
                .map(|sample| (call.id, sample, "pending_enter"))
        })
        .or_else(|| {
            tracker
                .latest_user
                .as_ref()
                .map(|(id, sample)| (*id, sample, "completed_enter"))
        });
    if let Some((id, sample, source)) = selected {
        use super::{symbol, unwind_fp};
        let r = sample.registers.0;
        let (mut frames, stop) = unwind_fp::walk(r[8], r[7], r[6], &mappings, |bp| {
            let offset = usize::try_from(bp.checked_sub(r[7])?).ok()?;
            sample
                .stack
                .get(offset..offset.checked_add(16)?)?
                .try_into()
                .ok()
        });
        let mut cache = symbol::ElfCache::default();
        for (index, frame) in frames.iter_mut().enumerate() {
            let address = symbol::instruction_address(frame.address, index > 0);
            if let Some(info) = mappings
                .iter()
                .find(|map| map.contains(address) && map.inode != 0)
                .and_then(|map| {
                    let elf = cache.get(tid, map)?;
                    symbol::resolve_frame(
                        symbol::elf_address(
                            address,
                            map,
                            &elf,
                            crate::http_server::process::procfs::page_size(),
                        )?,
                        &elf,
                    )
                })
            {
                frame.symbol = info.name;
            }
        }
        summary["user_sample"] = serde_json::json!({"syscall":syscall_name(id),"id":id,"rip":format!("0x{:x}",r[8]),"rsp":format!("0x{:x}",r[7]),"rbp":format!("0x{:x}",r[6]),"stack_bytes":sample.stack.len(),"sample_source":source,"sample_time_ns":sample.time_ns,"sample_age_ms":clock_ns(libc::CLOCK_MONOTONIC).saturating_sub(sample.time_ns)/1_000_000,"frames":frames,"unwind_stop":stop});
    }
    eprintln!("SYSCALL_RESULT {}", serde_json::to_string(&summary)?);
    Ok(())
}
fn clock_ns(clock: libc::clockid_t) -> u64 {
    let mut time: libc::timespec = unsafe { std::mem::zeroed() };
    unsafe {
        libc::clock_gettime(clock, &mut time);
    }
    time.tv_sec as u64 * 1_000_000_000 + time.tv_nsec as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    fn event(tid: i32, time_ns: u64, id: i64, ret: Option<i64>) -> Call {
        Call {
            tid,
            time_ns,
            id,
            args: None,
            ret,
            user: None,
        }
    }
    #[test]
    fn matches_per_tid_and_keeps_initial_exits_unknown() {
        let mut tracker = Tracker::default();
        tracker.ingest(event(1, 1, 230, Some(0)));
        tracker.ingest(event(1, 10, 202, None));
        tracker.ingest(event(2, 11, 232, None));
        tracker.ingest(event(1, 30, 202, Some(-110)));
        tracker.ingest(event(2, 41, 232, Some(0)));
        assert_eq!(tracker.unmatched_exit, 1);
        assert_eq!(tracker.durations[&202].sum, 20);
        assert_eq!(tracker.durations[&202].last_ret, -110);
        assert_eq!(tracker.durations[&232].sum, 30);
        assert!(tracker.pending.is_empty());
    }
    #[test]
    fn loss_mismatch_overwrite_and_late_records_do_not_invent_waits() {
        let mut tracker = Tracker::default();
        tracker.ingest(event(1, 10, 202, None));
        tracker.invalidate();
        tracker.ingest(event(1, 20, 202, Some(0)));
        assert!(tracker.durations.is_empty());
        assert!(tracker.pending.is_empty());
        tracker.ingest(event(1, 30, 202, None));
        tracker.ingest(event(1, 40, 232, Some(0)));
        assert!(tracker.pending.is_empty());
        tracker.ingest(event(1, 50, 202, None));
        tracker.ingest(event(1, 60, 202, None));
        tracker.ingest(event(1, 70, 202, Some(-512)));
        assert_eq!(tracker.overwritten_enter, 1);
        assert_eq!(tracker.durations[&202].sum, 10);
        tracker.ingest(event(1, 80, 202, None));
        tracker.ingest(event(1, 75, 202, Some(0)));
        assert_eq!(tracker.order_errors, 1);
        assert!(tracker.pending.is_empty());
    }
    #[test]
    fn raw_alignment_register_order_stack_bounds_and_abi_none() {
        let mut bytes = Vec::new();
        bytes.extend(1u32.to_ne_bytes());
        bytes.extend(1i32.to_ne_bytes());
        bytes.extend(123u64.to_ne_bytes());
        bytes.extend(7u64.to_ne_bytes());
        bytes.extend(68u32.to_ne_bytes());
        let mut raw = vec![0; 68];
        raw[..2].copy_from_slice(&384u16.to_ne_bytes());
        raw[4..8].copy_from_slice(&1i32.to_ne_bytes());
        raw[8..16].copy_from_slice(&202i64.to_ne_bytes());
        for index in 0..6 {
            raw[16 + index * 8..24 + index * 8].copy_from_slice(&(index as u64 + 10).to_ne_bytes());
        }
        bytes.extend(raw);
        bytes.extend(2u64.to_ne_bytes());
        for index in 0..24 {
            if REGS_MASK & (1 << index) != 0 {
                bytes.extend((index as u64 + 100).to_ne_bytes());
            }
        }
        bytes.extend(16u64.to_ne_bytes());
        bytes.extend([1; 16]);
        bytes.extend(8u64.to_ne_bytes());
        let parsed = call(&bytes, true, true, 384).unwrap();
        assert_eq!(parsed.args.unwrap(), [10, 11, 12, 13, 14, 15]);
        let user = parsed.user.unwrap();
        assert_eq!(user.registers.0[8], 108);
        assert_eq!(user.stack.len(), 8);
        for length in 0..bytes.len() {
            assert!(call(&bytes[..length], true, true, 384).is_err());
        }
        let mut absent = bytes[..104].to_vec();
        absent[96..104].fill(0);
        let parsed = call(&absent, true, true, 384).unwrap();
        assert!(parsed.user.is_none());
        assert_eq!(parsed.id, 202);
        assert!(call(&bytes, true, true, 383).is_err());
        let end = bytes.len();
        bytes[end - 8..].copy_from_slice(&17u64.to_ne_bytes());
        assert!(call(&bytes, true, true, 384).is_err());
    }
}

#[test]
#[ignore = "context-switch comparison for #109; requires dev_run.sh and target TID"]
fn syscall_context_baseline() -> Result<()> {
    let tid: i32 = std::env::var("PROCINSH_SYSCALL_TID")?.parse()?;
    ensure!(tid > 0, "TID must be positive");
    let size: u32 = std::env::var("PROCINSH_SYSCALL_STACK")?.parse()?;
    ensure!([2048, 4096, 8192].contains(&size), "invalid stack size");
    let seconds: u64 = std::env::var("PROCINSH_SYSCALL_SECONDS")?.parse()?;
    let mut event = Event::context_switch(tid, size)?;
    let mut counts = [0u64; 3];
    let mut bytes = 0u64;
    let mut latest = None;
    let start = std::time::Instant::now();
    let cpu = clock_ns(libc::CLOCK_PROCESS_CPUTIME_ID);
    while start.elapsed() < std::time::Duration::from_secs(seconds) {
        event.records(|kind, misc, payload| {
            bytes += payload.len() as u64 + 8;
            if kind == 9 {
                counts[0] += 1;
                ensure!(payload.len() >= 24, "truncated context sample");
                latest = user_sample(
                    i32::from_ne_bytes(payload[4..8].try_into()?),
                    u64::from_ne_bytes(payload[8..16].try_into()?),
                    u32::from_ne_bytes(payload[16..20].try_into()?),
                    &payload[24..],
                )?;
            }
            if kind == 14 && misc & (1 << 13) != 0 {
                counts[if misc & (1 << 14) == 0 { 1 } else { 2 }] += 1;
            }
            Ok(())
        })?;
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    eprintln!(
        "SYSCALL_RESULT {}",
        serde_json::json!({"context_samples":counts[0],"voluntary_out":counts[1],"preempt_out":counts[2],"context_lost":event.lost,"ring_bytes":bytes,"observer_cpu_ns":clock_ns(libc::CLOCK_PROCESS_CPUTIME_ID).saturating_sub(cpu),"elapsed_s":start.elapsed().as_secs_f64(),"has_user_sample":latest.is_some(),"stack_bytes":latest.as_ref().map(|s|s.stack.len())})
    );
    Ok(())
}
