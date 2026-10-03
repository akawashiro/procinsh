//! Opt-in experiment; never enabled by production collectors.
use super::{perf::Event, symbol, unwind_fp};
use crate::http_server::process::{maps, procfs};
use std::time::{Duration, Instant};

#[test]
#[ignore = "requires a target TID and perf permissions; see docs/CONTEXT_SWITCH_POC.md"]
fn context_switch_poc() -> anyhow::Result<()> {
    let tid: i32 = std::env::var("PROCINSH_POC_TID")?.parse()?;
    anyhow::ensure!(tid > 0, "TID must be positive");
    let stack: u32 = std::env::var("PROCINSH_POC_STACK")
        .unwrap_or("8192".into())
        .parse()?;
    anyhow::ensure!(
        [2048, 4096, 8192].contains(&stack),
        "stack must be 2048, 4096 or 8192"
    );
    let seconds: u64 = std::env::var("PROCINSH_POC_SECONDS")
        .unwrap_or("5".into())
        .parse()?;
    let records = std::env::var("PROCINSH_POC_SWITCH").as_deref() != Ok("0");
    let mut event = if std::env::var_os("PROCINSH_POC_CLOCK_ONLY").is_some() {
        Event::open(tid)?
    } else {
        Event::context_switch(tid, stack, records)?
    };
    let mut clock = if std::env::var_os("PROCINSH_POC_CLOCK").is_some() {
        Some(Event::open(tid)?)
    } else {
        None
    };
    let mappings = maps::read(tid, false)?;
    let mut cache = symbol::ElfCache::default();
    let cpu_start = cpu_ns();
    let start = Instant::now();
    let mut samples = 0u64;
    let mut abi_none = 0u64;
    let mut switch_in = 0u64;
    let mut voluntary = 0u64;
    let mut preempt = 0u64;
    let mut bytes = 0u64;
    let mut clock_samples = 0u64;
    let mut latest = None;
    let mut record_log = Vec::new();
    while start.elapsed() < Duration::from_secs(seconds) {
        if let Some(sample) = event.drain_records(|kind, misc, payload| {
            bytes += payload.len() as u64 + 8;
            if kind == 9 {
                samples += 1;
                if payload.get(24..32).is_some_and(|b| b == [0; 8]) {
                    abi_none += 1;
                }
            }
            if kind == 14 {
                if misc & (1 << 13) == 0 {
                    switch_in += 1;
                } else if misc & (1 << 14) != 0 {
                    preempt += 1;
                } else {
                    voluntary += 1;
                }
            }
            // Bounded raw sequence, including sample_id_all switch trailers.
            if record_log.len() < 32 && [9, 14].contains(&kind) {
                let timestamp = payload.get(8..16);
                record_log.push((
                    kind,
                    misc,
                    timestamp.map(|b| u64::from_ne_bytes(b.try_into().unwrap())),
                ));
            }
        })? {
            latest = Some(sample);
        }
        if let Some(clock) = &mut clock {
            clock.drain_records(|kind, _, _| {
                if kind == 9 {
                    clock_samples += 1;
                }
            })?;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    eprintln!("observer_cpu_ns={}", cpu_ns().saturating_sub(cpu_start));
    eprintln!(
        "elapsed={:?} samples={samples} abi_none={abi_none} switch_in={switch_in} voluntary_out={voluntary} preempt_out={preempt} ring_bytes={bytes} lost={} clock_samples={clock_samples} clock_lost={}",
        start.elapsed(),
        event.lost,
        clock.as_ref().map_or(0, |c| c.lost)
    );
    eprintln!("first_records(kind,misc,time_ns)={record_log:?}");
    if let Some(sample) = latest {
        let r = sample.registers.0;
        let (mut frames, stop) = unwind_fp::walk(r[8], r[7], r[6], &mappings, |bp| {
            let offset = usize::try_from(bp.checked_sub(r[7])?).ok()?;
            sample
                .stack
                .get(offset..offset.checked_add(16)?)?
                .try_into()
                .ok()
        });
        for (index, frame) in frames.iter_mut().enumerate() {
            let address = symbol::instruction_address(frame.address, index > 0);
            if let Some(info) = mappings
                .iter()
                .find(|m| m.contains(address) && m.inode != 0)
                .and_then(|map| {
                    let elf = cache.get(tid, map)?;
                    symbol::resolve_frame(
                        symbol::elf_address(address, map, &elf, procfs::page_size())?,
                        &elf,
                    )
                })
            {
                frame.symbol = info.name;
            }
        }
        eprintln!(
            "tid={} time_ns={} cpu={} RIP={:#x} RSP={:#x} RBP={:#x} stack_bytes={} stop={stop} frames={}",
            sample.tid,
            sample.time_ns,
            sample.cpu,
            r[8],
            r[7],
            r[6],
            sample.stack.len(),
            serde_json::to_string(&frames)?
        );
    } else {
        eprintln!(
            "No usable user sample; inspect samples/abi_none before attributing this to an already sleeping target."
        );
    }
    Ok(())
}

fn cpu_ns() -> u64 {
    let mut time: libc::timespec = unsafe { std::mem::zeroed() };
    unsafe {
        libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut time);
    }
    time.tv_sec as u64 * 1_000_000_000 + time.tv_nsec as u64
}
