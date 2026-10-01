use super::{self as process, threads};
use std::{
    io::{BufRead, BufReader},
    process::{Child, Command, Stdio},
    time::Duration,
};
pub(in crate::http_server) struct Target {
    pub(in crate::http_server) child: Child,
    pub(in crate::http_server) id: process::ProcessId,
    pub(in crate::http_server) address: u64,
}
impl Target {
    pub(in crate::http_server) fn new(name: &str) -> Self {
        super::super::test_support::build_targets();
        let mut child = Command::new(format!("tests/targets/bin/{name}"))
            .env("PROCINSH_TEST_ENV", "value=with\nline <b>literal</b>")
            .env("PROCINSH_TEST_EMPTY", "")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap())
            .read_line(&mut line)
            .unwrap();
        let address = line
            .split_whitespace()
            .nth(1)
            .and_then(|v| u64::from_str_radix(v.trim_start_matches("0x"), 16).ok())
            .unwrap_or(0);
        let id = process::identity(child.id() as i32).unwrap();
        std::thread::sleep(Duration::from_millis(30));
        Self { child, id, address }
    }

    pub(in crate::http_server) fn assert_detached(&self) {
        for tid in threads::tids(self.id.pid).unwrap() {
            if let Ok(status) =
                process::procfs::fields(&format!("/proc/{}/task/{tid}/status", self.id.pid))
            {
                assert_eq!(
                    process::procfs::field_u64(&status, "TracerPid"),
                    Some(0),
                    "TID {tid} remained traced"
                );
                assert!(
                    !status["State"].starts_with('t'),
                    "TID {tid} remained in ptrace stop"
                );
            }
        }
    }
}
impl Drop for Target {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
