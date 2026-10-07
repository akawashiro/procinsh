//! Loads kernel-specific Aya sensors and owns their attached program links.
//!
//! # Interface
//!
//! - [`load`] (`pub(super) fn load(name: &str) -> anyhow::Result<aya::Ebpf>`):
//!   checks BTF compatibility before creating any BPF resources, then attaches a sensor.
//! - [`wire`] (`pub(super) mod`): shared fixed-layout map/event types; see its definitions.
use anyhow::{Context, Result, bail, ensure};
use aya::{
    Btf, Ebpf,
    programs::{BtfTracePoint, FEntry, FExit},
};
use sha2::{Digest, Sha256};
#[path = "../../ebpf/src/wire.rs"]
#[allow(dead_code)]
pub(super) mod wire;
// SAFETY: these repr(C) types consist solely of integers, have explicit padding,
// and accept every bit pattern. Layout assertions live in the shared definition.
unsafe impl aya::Pod for wire::ProcessKey {}
unsafe impl aya::Pod for wire::CpuSlot {}
unsafe impl aya::Pod for wire::CpuTotal {}
include!(concat!(env!("OUT_DIR"), "/kernel_btf_digest.rs"));

fn verify_btf(bytes: &[u8], expected: &[u8; 32]) -> Result<()> {
    let actual: [u8; 32] = Sha256::digest(bytes).into();
    ensure!(
        &actual == expected,
        "kernel BTF differs from the build kernel; rebuild on this kernel (checkout: cargo clean -p procinsh && cargo build --locked; installed: cargo install procinsh --locked --force)"
    );
    Ok(())
}
enum Hook {
    Entry,
    Exit,
    TracePoint,
}

/// Aya stores links in the programs owned by Ebpf. Dropping a partially loaded
/// object detaches all successful attachments as well as releasing its maps.
/// BTF is compared in full, conservatively rejecting even unrelated changes.
pub(super) fn load(name: &str) -> Result<Ebpf> {
    let bytes = std::fs::read("/sys/kernel/btf/vmlinux")
        .context("cannot read kernel BTF; rebuild and run with readable /sys/kernel/btf/vmlinux")?;
    verify_btf(&bytes, &BUILD_BTF_DIGEST)?;
    let btf = Btf::parse(&bytes, aya::Endianness::Little)?;
    use Hook::*;
    let (object, hooks): (&[u8], &[(&str, &str, Hook)]) = match name {
        "sched" => (
            aya::include_bytes_aligned!(concat!(env!("OUT_DIR"), "/sched.bpf.o")),
            &[
                ("schedule", "sched_switch", TracePoint),
                ("process_exit", "sched_process_exit", TracePoint),
            ],
        ),
        "ipc" => (
            aya::include_bytes_aligned!(concat!(env!("OUT_DIR"), "/ipc.bpf.o")),
            &[
                ("anon_pipe_read", "anon_pipe_read", Exit),
                ("anon_pipe_write", "anon_pipe_write", Exit),
                ("send", "sock_send_length", TracePoint),
                ("recv", "sock_recv_length", TracePoint),
            ],
        ),
        "files" => (
            aya::include_bytes_aligned!(concat!(env!("OUT_DIR"), "/files.bpf.o")),
            &[
                ("file_path", "security_file_permission", Entry),
                ("enter_vfs_read", "vfs_read", Entry),
                ("exit_vfs_read", "vfs_read", Exit),
                ("enter_vfs_write", "vfs_write", Entry),
                ("exit_vfs_write", "vfs_write", Exit),
                ("enter_vfs_readv", "vfs_readv", Entry),
                ("exit_vfs_readv", "vfs_readv", Exit),
                ("enter_vfs_writev", "vfs_writev", Entry),
                ("exit_vfs_writev", "vfs_writev", Exit),
                ("file_exit", "sched_process_exit", TracePoint),
            ],
        ),
        _ => bail!("unknown BPF sensor {name}"),
    };
    let mut bpf =
        Ebpf::load(object).context("CAP_BPF / CAP_PERFMON and compatible kernel hooks required")?;
    for (program, target, kind) in hooks {
        let p = bpf
            .program_mut(program)
            .with_context(|| format!("missing program {program}"))?;
        let mut attach = || -> Result<()> {
            match kind {
                Entry => {
                    let p: &mut FEntry = p.try_into()?;
                    p.load(target, &btf)?;
                    p.attach()?;
                }
                Exit => {
                    let p: &mut FExit = p.try_into()?;
                    p.load(target, &btf)?;
                    p.attach()?;
                }
                TracePoint => {
                    let p: &mut BtfTracePoint = p.try_into()?;
                    p.load(target, &btf)?;
                    p.attach()?;
                }
            }
            Ok(())
        };
        attach().with_context(|| format!("load/attach {program} to {target}"))?;
    }
    Ok(bpf)
}
#[cfg(test)]
fn has_bpf_capabilities() -> bool {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let bits = status
        .lines()
        .find_map(|line| line.strip_prefix("CapEff:\t"))
        .unwrap();
    let bits = u64::from_str_radix(bits, 16).unwrap();
    bits & ((1 << 39) | (1 << 38)) == ((1 << 39) | (1 << 38)) || bits & (1 << 21) != 0
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn btf_digest_rejects_different_kernel() {
        let digest: [u8; 32] = Sha256::digest(b"kernel one").into();
        verify_btf(b"kernel one", &digest).unwrap();
        let error = verify_btf(b"kernel two", &digest).unwrap_err().to_string();
        assert!(error.contains("rebuild"));
    }
    #[test]
    fn build_kernel_matches_running_kernel() {
        verify_btf(
            &std::fs::read("/sys/kernel/btf/vmlinux").unwrap(),
            &BUILD_BTF_DIGEST,
        )
        .unwrap();
    }
    #[test]
    fn all_sensors_load_and_detach() {
        // Run through scripts/dev_test.sh; unprivileged cargo test may skip.
        if !has_bpf_capabilities() {
            assert!(
                std::env::var("PROCINSH_REQUIRE_BPF").as_deref() != Ok("1"),
                "BPF capabilities required; run scripts/dev_test.sh"
            );
            eprintln!("BPF load test skipped: run PROCINSH_REQUIRE_BPF=1 scripts/dev_test.sh");
            return;
        }
        for name in ["files", "sched", "ipc"] {
            drop(load(name).unwrap());
        }
    }
}
