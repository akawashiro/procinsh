use anyhow::{Context, Result};
use std::io;

// Linux capability ABI v3 covers capabilities 0..63 on x86-64.
#[repr(C)]
struct CapabilityHeader {
    version: u32,
    pid: i32,
}
#[derive(Clone, Copy, Default)]
#[repr(C)]
struct CapabilitySet {
    effective: u32,
    permitted: u32,
    inheritable: u32,
}
const REQUIRED: u64 = (1 << 2) | (1 << 19) | (1 << 21) | (1 << 38) | (1 << 39);

fn check(result: libc::c_long, operation: &str) -> Result<()> {
    if result < 0 {
        return Err(io::Error::last_os_error()).context(operation.to_owned());
    }
    Ok(())
}
fn capabilities() -> Result<[CapabilitySet; 2]> {
    let mut header = CapabilityHeader {
        version: 0x20080522,
        pid: 0,
    };
    let mut data = [CapabilitySet::default(); 2];
    check(
        unsafe { libc::syscall(libc::SYS_capget, &mut header, data.as_mut_ptr()) },
        "capget",
    )?;
    Ok(data)
}
fn set_capabilities(data: &[CapabilitySet; 2]) -> Result<()> {
    let header = CapabilityHeader {
        version: 0x20080522,
        pid: 0,
    };
    check(
        unsafe { libc::syscall(libc::SYS_capset, &header, data.as_ptr()) },
        "capset",
    )
}
fn prctl(option: i32, arg: libc::c_ulong, operation: &str) -> Result<()> {
    check(
        unsafe { libc::prctl(option, arg, 0, 0, 0) } as libc::c_long,
        operation,
    )
}

fn clear_ambient() -> Result<()> {
    prctl(
        libc::PR_CAP_AMBIENT,
        libc::PR_CAP_AMBIENT_CLEAR_ALL as _,
        "clear ambient capabilities",
    )
}

pub(crate) fn restrict_monitor_privileges() -> Result<()> {
    clear_ambient()?;
    let mut data = capabilities()?;
    if data[0].effective & (1 << 8) != 0 {
        // Disable UID-0's special exec capability rules while CAP_SETPCAP is
        // available; keep only the five explicitly documented inspection caps.
        prctl(libc::PR_SET_SECUREBITS, 1 | 2, "lock SECBIT_NOROOT")?;
        let last: u32 = std::fs::read_to_string("/proc/sys/kernel/cap_last_cap")?
            .trim()
            .parse()?;
        for cap in 0..=last {
            if cap >= 64 || REQUIRED & (1_u64 << cap) == 0 {
                prctl(
                    libc::PR_CAPBSET_DROP,
                    cap as _,
                    "restrict monitor bounding set",
                )?;
            }
        }
    }
    for (i, set) in data.iter_mut().enumerate() {
        let mask = (REQUIRED >> (i * 32)) as u32;
        set.permitted &= mask;
        set.effective &= mask;
        set.inheritable = 0;
    }
    set_capabilities(&data)?;
    prctl(libc::PR_SET_NO_NEW_PRIVS, 1, "set monitor no_new_privs")?;
    prctl(libc::PR_SET_DUMPABLE, 0, "disable monitor dumps")
}

pub(crate) fn effective_capabilities() -> Result<u64> {
    let sets = capabilities()?;
    Ok(sets[0].effective as u64 | ((sets[1].effective as u64) << 32))
}
