use super::{ProcessId, check_identity, timestamp_ms};
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::{
    fs,
    time::{Duration, Instant},
};

#[derive(Debug, Serialize)]
pub struct Mask {
    pub hex: String,
    pub signals: Vec<String>,
}
#[derive(Debug, Serialize)]
pub struct SignalStatus {
    pub tid: i32,
    pub name: String,
    pub pending: Mask,
    pub shared_pending: Mask,
    pub blocked: Mask,
    pub ignored: Mask,
    pub caught: Mask,
    pub queued: String,
}
#[derive(Serialize)]
pub struct Signals {
    pub process_id: ProcessId,
    pub captured_at: u64,
    pub leader: SignalStatus,
    pub threads: Vec<SignalStatus>,
    pub warnings: Vec<String>,
}
fn mask(value: &str) -> Result<Mask> {
    let bits = u64::from_str_radix(value, 16).context("Invalid signal mask")?;
    const NAMES: [&str; 31] = [
        "SIGHUP",
        "SIGINT",
        "SIGQUIT",
        "SIGILL",
        "SIGTRAP",
        "SIGABRT",
        "SIGBUS",
        "SIGFPE",
        "SIGKILL",
        "SIGUSR1",
        "SIGSEGV",
        "SIGUSR2",
        "SIGPIPE",
        "SIGALRM",
        "SIGTERM",
        "SIGSTKFLT",
        "SIGCHLD",
        "SIGCONT",
        "SIGSTOP",
        "SIGTSTP",
        "SIGTTIN",
        "SIGTTOU",
        "SIGURG",
        "SIGXCPU",
        "SIGXFSZ",
        "SIGVTALRM",
        "SIGPROF",
        "SIGWINCH",
        "SIGIO",
        "SIGPWR",
        "SIGSYS",
    ];
    let signals = (1..=64)
        .filter(|n| bits & (1u64 << (n - 1)) != 0)
        .map(|n| {
            let name = if n <= 31 {
                NAMES[n - 1].to_string()
            } else {
                format!("RT (kernel {})", n)
            };
            format!("{name} [{n}]")
        })
        .collect();
    Ok(Mask {
        hex: format!("0x{bits:016x}"),
        signals,
    })
}
fn parse(tid: i32, text: &str) -> Result<SignalStatus> {
    let field = |key: &str| -> Result<&str> {
        text.lines()
            .find_map(|line| {
                line.split_once(':')
                    .filter(|(k, _)| *k == key)
                    .map(|(_, v)| v.trim())
            })
            .with_context(|| format!("Missing {key}"))
    };
    ensure!(
        field("Pid")?.parse::<i32>()? == tid,
        "Thread identity changed"
    );
    Ok(SignalStatus {
        tid,
        name: field("Name")?.to_string(),
        pending: mask(field("SigPnd")?)?,
        shared_pending: mask(field("ShdPnd")?)?,
        blocked: mask(field("SigBlk")?)?,
        ignored: mask(field("SigIgn")?)?,
        caught: mask(field("SigCgt")?)?,
        queued: field("SigQ")?.to_string(),
    })
}
pub fn read(id: ProcessId) -> Result<Signals> {
    check_identity(id)?;
    let leader = parse(
        id.pid,
        &fs::read_to_string(format!("/proc/{}/status", id.pid))?,
    )?;
    let mut threads = Vec::new();
    let mut warnings = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(2);
    for (count, entry) in fs::read_dir(format!("/proc/{}/task", id.pid))?.enumerate() {
        if count >= 4096 || Instant::now() >= deadline {
            warnings.push("The thread limit was reached. Showing partial results.".into());
            break;
        }
        let result = (|| -> Result<SignalStatus> {
            let path = entry?.path();
            let tid = path
                .file_name()
                .context("Missing TID")?
                .to_string_lossy()
                .parse()?;
            parse(tid, &fs::read_to_string(path.join("status"))?)
        })();
        match result {
            Ok(thread) => threads.push(thread),
            Err(e) => warnings.push(format!(
                "Could not read thread information (the thread may have exited or access may be denied): {e}"
            )),
        }
    }
    threads.sort_by_key(|t| t.tid);
    check_identity(id)?;
    Ok(Signals {
        process_id: id,
        captured_at: timestamp_ms(),
        leader,
        threads,
        warnings,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn masks_preserve_high_bit_and_signal_numbers() {
        let m = mask("8000000000000200").unwrap();
        assert_eq!(m.signals, ["SIGUSR1 [10]", "RT (kernel 64) [64]"]);
        assert_eq!(m.hex, "0x8000000000000200");
        assert!(mask("0").unwrap().signals.is_empty());
        assert!(mask("not hex").is_err());
    }
    #[test]
    fn reads_live_threads_and_rejects_reused_identity() {
        let id = super::super::identity(std::process::id() as i32).unwrap();
        let data = read(id).unwrap();
        assert!(data.threads.iter().any(|t| t.tid == id.pid));
        assert_eq!(data.leader.tid, id.pid);
        assert!(
            read(ProcessId {
                start_time_ticks: id.start_time_ticks + 1,
                ..id
            })
            .is_err()
        );
        assert!(parse(1, "Name:\ttest\nPid:\t1\n").is_err());
    }
}
