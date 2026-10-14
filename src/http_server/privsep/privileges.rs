use anyhow::{Context, Result, ensure};
use std::{io, os::fd::RawFd};

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

/// Close inherited resources before creating the socketpair or any runtime.
/// Preserve only stdio. Enumeration is collected before closing its directory.
pub(super) fn close_inherited() -> Result<()> {
    if unsafe { libc::syscall(libc::SYS_close_range, 3_u32, u32::MAX, 0_u32) } == 0 {
        return Ok(());
    }
    let fds: Vec<RawFd> = std::fs::read_dir("/proc/self/fd")?
        .filter_map(|entry| entry.ok()?.file_name().to_str()?.parse().ok())
        .collect();
    for fd in fds.into_iter().filter(|fd| *fd >= 3) {
        unsafe { libc::close(fd) };
    }
    Ok(())
}

/// Resolve credentials while still single-threaded. A direct root launch uses
/// nobody; sudo launches use the invoking user's UID/GID. Invalid sudo IDs fail
/// closed rather than leaving a root HTTP worker.
pub(super) fn application_identity() -> Result<(u32, u32)> {
    let uid = unsafe { libc::getuid() };
    let gid = unsafe { libc::getgid() };
    let identity = if uid != 0 {
        (uid, gid)
    } else {
        match (std::env::var("SUDO_UID"), std::env::var("SUDO_GID")) {
            (Ok(uid), Ok(gid)) => (
                uid.parse().context("invalid SUDO_UID")?,
                gid.parse().context("invalid SUDO_GID")?,
            ),
            (Err(std::env::VarError::NotPresent), Err(std::env::VarError::NotPresent)) => {
                (65534, 65534)
            }
            _ => anyhow::bail!("SUDO_UID and SUDO_GID must both be valid non-root IDs"),
        }
    };
    ensure!(
        identity.0 != 0 && identity.1 != 0 && identity.0 != u32::MAX && identity.1 != u32::MAX,
        "HTTP worker requires a non-root UID and GID"
    );
    Ok(identity)
}

fn clear_ambient() -> Result<()> {
    prctl(
        libc::PR_CAP_AMBIENT,
        libc::PR_CAP_AMBIENT_CLEAR_ALL as _,
        "clear ambient capabilities",
    )
}

pub(super) fn drop_application_privileges(uid: u32, gid: u32) -> Result<()> {
    prctl(libc::PR_SET_NO_NEW_PRIVS, 1, "set no_new_privs")?;
    clear_ambient()?;
    let data = capabilities()?;
    if data[0].effective & (1 << 8) != 0 {
        // CAP_SETPCAP is available on a conventional sudo launch. File-capability
        // launches may lack it; no_new_privs still prevents exec privilege gains.
        let last: u32 = std::fs::read_to_string("/proc/sys/kernel/cap_last_cap")?
            .trim()
            .parse()?;
        for cap in 0..=last {
            prctl(libc::PR_CAPBSET_DROP, cap as _, "drop bounding capability")?;
        }
    }
    if unsafe { libc::geteuid() } == 0 || data[0].effective & (1 << 6) != 0 {
        check(
            unsafe { libc::setgroups(0, std::ptr::null()) } as _,
            "clear supplementary groups",
        )?;
    }
    check(unsafe { libc::setresgid(gid, gid, gid) } as _, "drop GIDs")?;
    check(unsafe { libc::setresuid(uid, uid, uid) } as _, "drop UIDs")?;
    set_capabilities(&[CapabilitySet::default(); 2])?;
    let (mut real_uid, mut effective_uid, mut saved_uid) = (0, 0, 0);
    let (mut real_gid, mut effective_gid, mut saved_gid) = (0, 0, 0);
    check(
        unsafe { libc::getresuid(&mut real_uid, &mut effective_uid, &mut saved_uid) } as _,
        "verify UIDs",
    )?;
    check(
        unsafe { libc::getresgid(&mut real_gid, &mut effective_gid, &mut saved_gid) } as _,
        "verify GIDs",
    )?;
    ensure!(
        [real_uid, effective_uid, saved_uid] == [uid; 3]
            && [real_gid, effective_gid, saved_gid] == [gid; 3],
        "HTTP worker retained elevated IDs"
    );
    // Do not let ordinary same-UID processes inspect or tamper with the IPC
    // client. This also closes the /proc/PID/fd access route to the socketpair.
    prctl(libc::PR_SET_DUMPABLE, 0, "disable HTTP worker dumps")?;
    let data = capabilities()?;
    ensure!(
        data.iter()
            .all(|s| s.permitted == 0 && s.effective == 0 && s.inheritable == 0),
        "HTTP worker retained capabilities"
    );
    Ok(())
}

pub(super) fn restrict_monitor_privileges(uid: u32) -> Result<()> {
    clear_ambient()?;
    // Matching the worker's real UID permits lifecycle signals after dropping
    // CAP_KILL. Keep the effective UID unchanged so PDEATHSIG is not triggered
    // by a parent credential transition during worker startup.
    if unsafe { libc::getuid() } == 0 {
        check(
            unsafe { libc::setresuid(uid, u32::MAX, u32::MAX) } as _,
            "set monitor real UID",
        )?;
    }
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
