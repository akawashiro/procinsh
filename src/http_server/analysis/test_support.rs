//! Local fixture acquisition for unprivileged-analysis tests.
pub(super) mod process {
    use crate::{
        http_server::analysis::snapshot::symbol::{ElfCache, ElfFile, ElfSymbols},
        http_server::model::memory::MemoryMap,
        ipc::schema::ProcessId,
    };
    use std::{
        fs,
        io::{BufRead, BufReader},
        os::unix::fs::MetadataExt,
        process::{Child, Command, Stdio},
        sync::Arc,
    };
    pub(crate) mod procfs {
        pub fn page_size() -> u64 {
            (unsafe { libc::sysconf(libc::_SC_PAGESIZE) }) as u64
        }
    }
    pub(crate) mod maps {
        pub use crate::http_server::model::memory::parse_map;
        pub fn read_maps(
            pid: i32,
        ) -> anyhow::Result<Vec<crate::http_server::model::memory::MemoryMap>> {
            fs::read_to_string(format!("/proc/{pid}/maps"))?
                .lines()
                .map(parse_map)
                .collect()
        }
        use std::fs;
    }
    pub fn load(cache: &mut ElfCache, pid: i32, map: &MemoryMap) -> Option<Arc<ElfSymbols>> {
        let path = format!("/proc/{pid}/root{}", map.pathname.as_deref()?);
        let meta = fs::metadata(&path).ok()?;
        cache.insert(
            map,
            ElfFile {
                identity: (
                    meta.dev(),
                    meta.ino(),
                    meta.len(),
                    meta.mtime(),
                    meta.mtime_nsec(),
                ),
                bytes: fs::read(path).ok()?,
            },
        )
    }
    /// Stop a fixture while copying native registers and a bounded stack. The
    /// guard also detaches if test acquisition fails or an assertion unwinds.
    pub fn capture_registers(
        tid: i32,
        maps: &[MemoryMap],
    ) -> anyhow::Result<(crate::ipc::registers::RegisterSet, Vec<u8>)> {
        use crate::ipc::registers::RegisterSet;
        use anyhow::ensure;
        fn ptrace(op: libc::c_uint, tid: i32, addr: usize, data: usize) -> anyhow::Result<()> {
            ensure!(
                unsafe {
                    libc::ptrace(
                        op,
                        tid,
                        addr as *mut libc::c_void,
                        data as *mut libc::c_void,
                    )
                } != -1,
                "ptrace: {}",
                std::io::Error::last_os_error()
            );
            Ok(())
        }
        struct Attachment(i32);
        impl Drop for Attachment {
            fn drop(&mut self) {
                let _ = ptrace(libc::PTRACE_DETACH, self.0, 0, 0);
            }
        }
        ptrace(libc::PTRACE_ATTACH, tid, 0, 0)?;
        let _attachment = Attachment(tid);
        let mut status = 0;
        ensure!(
            unsafe { libc::waitpid(tid, &mut status, libc::__WALL) } == tid
                && libc::WIFSTOPPED(status),
            "fixture did not stop"
        );
        // NT_PRSTATUS layout differs from the perf layout only on x86-64.
        let mut raw = [0u64; 34];
        #[cfg(target_arch = "aarch64")]
        let expected = 34 * 8;
        #[cfg(target_arch = "x86_64")]
        let expected = std::mem::size_of::<libc::user_regs_struct>();
        let mut iov = libc::iovec {
            iov_base: raw.as_mut_ptr().cast(),
            iov_len: expected,
        };
        ptrace(
            libc::PTRACE_GETREGSET,
            tid,
            libc::NT_PRSTATUS as usize,
            std::ptr::from_mut(&mut iov) as usize,
        )?;
        ensure!(iov.iov_len == expected, "incomplete fixture registers");
        let mut r = RegisterSet {
            flags_available: true,
            ..RegisterSet::default()
        };
        #[cfg(target_arch = "aarch64")]
        r.values.copy_from_slice(&raw);
        #[cfg(target_arch = "x86_64")]
        {
            let regs =
                unsafe { std::ptr::read_unaligned(raw.as_ptr().cast::<libc::user_regs_struct>()) };
            r.values[..10].copy_from_slice(&[
                regs.rax,
                regs.rbx,
                regs.rcx,
                regs.rdx,
                regs.rsi,
                regs.rdi,
                regs.rbp,
                regs.rsp,
                regs.rip,
                regs.eflags,
            ]);
            r.values[16..].copy_from_slice(&[
                regs.r8, regs.r9, regs.r10, regs.r11, regs.r12, regs.r13, regs.r14, regs.r15,
            ]);
        }
        let map = maps.iter().find(|m| m.contains(r.sp())).unwrap();
        let mut stack = vec![0u8; (map.end - r.sp()).min(16384) as usize];
        let local = libc::iovec {
            iov_base: stack.as_mut_ptr().cast(),
            iov_len: stack.len(),
        };
        let remote = libc::iovec {
            iov_base: r.sp() as *mut libc::c_void,
            iov_len: stack.len(),
        };
        let length = unsafe { libc::process_vm_readv(tid, &local, 1, &remote, 1, 0) };
        ensure!(length >= 0, "fixture stack read failed");
        stack.truncate(length as usize);
        Ok((r, stack))
    }
    pub struct TestTarget {
        pub id: ProcessId,
        child: Child,
    }
    impl TestTarget {
        pub fn new(name: &str) -> Self {
            crate::test_support::build_targets();
            let mut child = Command::new(format!("tests/targets/bin/{name}"))
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            let mut line = String::new();
            BufReader::new(child.stdout.take().unwrap())
                .read_line(&mut line)
                .unwrap();
            let pid = child.id() as i32;
            let stat = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap();
            let start = stat[stat.rfind(')').unwrap() + 1..]
                .split_whitespace()
                .nth(19)
                .unwrap()
                .parse()
                .unwrap();
            Self {
                id: ProcessId {
                    pid,
                    start_time_ticks: start,
                },
                child,
            }
        }
        pub fn assert_detached(&self) {
            for tid in fs::read_dir(format!("/proc/{}/task", self.id.pid))
                .unwrap()
                .flatten()
            {
                let status = fs::read_to_string(tid.path().join("status")).unwrap();
                assert!(status.lines().any(|line| line.trim() == "TracerPid:\t0"));
            }
        }
    }
    impl Drop for TestTarget {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
