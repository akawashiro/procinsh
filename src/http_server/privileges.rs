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

fn clear_ambient() -> Result<()> {
    prctl(
        libc::PR_CAP_AMBIENT,
        libc::PR_CAP_AMBIENT_CLEAR_ALL as _,
        "clear ambient capabilities",
    )
}

pub(super) fn drop_application_capabilities() -> Result<()> {
    clear_ambient()?;
    set_capabilities(&[CapabilitySet::default(); 2])?;
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

pub(super) fn seal_application() -> Result<()> {
    prctl(libc::PR_SET_NO_NEW_PRIVS, 1, "set application no_new_privs")
}
