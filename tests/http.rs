//! Exercise the installed interface: start the binary and communicate over HTTP.
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpStream},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

// Keep the observed process ordinary even when the server has file capabilities.
struct Target(Child);
impl Drop for Target {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct Server {
    child: Child,
    address: SocketAddr,
    second_address: SocketAddr,
}
impl Server {
    fn start() -> Self {
        Self::start_with_privileges(true)
    }

    fn start_with_privileges(privileged: bool) -> Self {
        let mut command = Command::new(
            std::env::var_os("PROCINSH_BINARY")
                .unwrap_or_else(|| env!("CARGO_BIN_EXE_procinsh").into()),
        );
        command
            .args(["--listen", "127.0.0.1:0", "--listen", "[::1]:0"])
            .env("RUST_LOG", "info")
            .stderr(Stdio::piped());
        if !privileged {
            use std::os::unix::process::CommandExt;
            // Only an async-signal-safe syscall runs between fork and exec.
            unsafe {
                command.pre_exec(|| {
                    if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }
        let mut child = command.spawn().unwrap();
        let stderr = child.stderr.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                eprintln!("{line}");
                if let Some(address) = line.split("listening on http://").nth(1) {
                    let _ = tx.send(address.split_whitespace().next().unwrap().to_owned());
                }
            }
        });
        // Install the cleanup guard before waiting for startup.
        let mut server = Self {
            child,
            address: "127.0.0.1:0".parse().unwrap(),
            second_address: "[::1]:0".parse().unwrap(),
        };
        server.address = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("server startup")
            .parse()
            .unwrap();
        server.second_address = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("second listener startup")
            .parse()
            .unwrap();
        server
    }

    fn connect(&self, path: &str, host: &str) -> BufReader<TcpStream> {
        let mut stream = TcpStream::connect_timeout(&self.address, Duration::from_secs(3)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        write!(stream, "GET {path} HTTP/1.0\r\nHost: {host}\r\n\r\n").unwrap();
        BufReader::new(stream)
    }

    fn get(&self, path: &str) -> (u16, String, String) {
        let mut response = String::new();
        self.connect(path, &self.address.to_string())
            .read_to_string(&mut response)
            .unwrap();
        let (headers, body) = response.split_once("\r\n\r\n").unwrap();
        let status = headers.split_whitespace().nth(1).unwrap().parse().unwrap();
        (status, headers.to_owned(), body.to_owned())
    }

    fn events(&self, path: &str, event: &str) -> BufReader<TcpStream> {
        let mut reader = self.connect(path, &self.address.to_string());
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        assert!(line.contains("200 OK"), "{line}");
        loop {
            line.clear();
            assert!(reader.read_line(&mut line).unwrap() > 0);
            if line.trim_end() == format!("event: {event}") {
                break;
            }
        }
        line.clear();
        reader.read_line(&mut line).unwrap();
        serde_json::from_str::<serde_json::Value>(line.trim().strip_prefix("data: ").unwrap())
            .unwrap();
        reader
    }

    fn shutdown(&mut self) {
        assert_eq!(
            unsafe { libc::kill(self.child.id() as i32, libc::SIGTERM) },
            0
        );
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "{status}");
                break;
            }
            assert!(Instant::now() < deadline, "graceful shutdown timed out");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn binary_serves_assets_process_api_and_sse_and_shuts_down() {
    let mut server = Server::start();
    let mut assets = std::collections::BTreeSet::new();
    for path in ["/", "/list", "/process/1", "/space", "/api/config"] {
        let (status, headers, body) = server.get(path);
        assert_eq!(status, 200, "{path}");
        assert!(headers.contains("cache-control: no-store"));
        assert!(!body.is_empty());
        if path != "/api/config" {
            assert!(headers.contains("text/html; charset=utf-8"));
            assert!(!body.contains("{{PROCINSH_"));
            for attribute in ["src=\"", "href=\""] {
                for value in body.split(attribute).skip(1) {
                    let url = value.split('"').next().unwrap();
                    if url.starts_with("/assets/") {
                        assets.insert(url.to_owned());
                    }
                }
            }
        }
    }
    assert!(assets.iter().any(|path| path.ends_with(".js")));
    assert!(assets.iter().any(|path| path.ends_with(".css")));
    for path in assets {
        let (status, headers, body) = server.get(&path);
        assert_eq!(status, 200, "{path}");
        assert!(!body.is_empty());
        if path.ends_with(".js") {
            assert!(headers.contains("javascript"), "{headers}");
        } else if path.ends_with(".css") {
            assert!(headers.contains("text/css; charset=utf-8"));
        }
    }
    let list = server.get("/list").2;
    let process = server.get("/process/1").2;
    let space = server.get("/space").2;
    assert!(list.contains("id=\"explorer\""));
    assert!(!list.contains("id=\"inspector\""));
    assert!(process.contains("id=\"inspector\""));
    assert!(!process.contains("id=\"explorer\""));
    assert!(space.contains("id=\"world\""));
    assert!(!space.contains("id=\"explorer\""));
    assert_eq!(server.get("/").2, space);
    for path in [
        "/app.js",
        "/display.js",
        "/style.css",
        "/space.js",
        "/space-model.js",
        "/space.css",
    ] {
        assert_eq!(server.get(path).0, 404, "{path}");
    }
    let mut denied = String::new();
    server
        .connect("/api/processes", "evil.test")
        .read_to_string(&mut denied)
        .unwrap();
    assert!(denied.starts_with("HTTP/1.0 403"));
    assert!(
        Command::new("sh")
            .arg("tests/targets/build.sh")
            .status()
            .unwrap()
            .success()
    );
    let mut target = Target(
        Command::new("tests/targets/bin/sleeping")
            .arg("--allow-inspector")
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut ready = String::new();
    BufReader::new(target.0.stdout.take().unwrap())
        .read_line(&mut ready)
        .unwrap();
    assert!(!ready.is_empty());
    let (status, _, body) = server.get("/api/processes");
    assert_eq!(status, 200);
    let processes: serde_json::Value = serde_json::from_str(&body).unwrap();
    let identity = &processes
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["identity"]["pid"] == target.0.id())
        .unwrap()["identity"];
    let pid = identity["pid"].as_u64().unwrap();
    let start = identity["start_time_ticks"].as_u64().unwrap();
    let query = format!("pid={pid}&start_time_ticks={start}");
    assert_eq!(server.get("/api/processes/signals").0, 404);
    assert_eq!(
        server.get(&format!("/api/processes/signals?{query}")).0,
        404
    );
    let monitor_caps =
        u64::from_str_radix(&status_fields(server.child.id())["CapPrm"], 16).unwrap();
    let target_caps = u64::from_str_radix(&status_fields(target.0.id())["CapPrm"], 16).unwrap();
    // Root containers may lack CAP_SYS_PTRACE. Restricting the monitor to the
    // inspection capabilities then makes the root fixture's other permitted
    // capabilities inaccessible under Linux's ptrace capability-superset rule.
    let restricted_root = unsafe { libc::geteuid() } == 0
        && monitor_caps & (1 << 19) == 0
        && target_caps & !monitor_caps != 0;
    for endpoint in [
        "observation",
        "threads",
        "maps",
        "environment",
        "auxv",
        "fds",
    ] {
        let (status, _, body) = server.get(&format!("/api/processes/{endpoint}?{query}"));
        let payload: serde_json::Value = serde_json::from_str(&body).unwrap();
        if restricted_root && ["environment", "auxv"].contains(&endpoint) {
            assert_eq!(status, 422, "{endpoint}: {body}");
            assert!(
                payload["error"]
                    .as_str()
                    .unwrap()
                    .contains("Permission denied"),
                "{body}"
            );
        } else {
            assert_eq!(status, 200, "{endpoint}: {body}");
            if restricted_root && endpoint == "fds" {
                assert!(payload["entries"].as_array().unwrap().is_empty());
                assert!(!payload["warnings"].as_array().unwrap().is_empty());
            }
            if restricted_root && endpoint == "maps" {
                assert!(payload["error"].is_string());
            }
        }
        assert_eq!(
            server
                .get(&format!(
                    "/api/processes/{endpoint}?pid={pid}&start_time_ticks={}",
                    start + 1
                ))
                .0,
            410
        );
        assert_eq!(server.get(&format!("/api/processes/{endpoint}")).0, 400);
    }
    let process = server.events(&format!("/api/processes/events?{query}"), "observation");
    let system = server.events("/api/system/events", "snapshot");
    server.address = server.second_address;
    assert_eq!(server.get("/api/config").0, 200);
    let second_system = server.events("/api/system/events", "snapshot");
    // Consume queued events while shutdown drains all open streams.
    let readers = [process, system, second_system]
        .map(|mut reader| std::thread::spawn(move || reader.read_to_end(&mut Vec::new()).unwrap()));
    server.shutdown();
    for reader in readers {
        reader.join().unwrap();
    }
}

fn status_fields(pid: u32) -> std::collections::HashMap<String, String> {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .unwrap()
        .lines()
        .filter_map(|line| {
            line.split_once(':')
                .map(|(key, value)| (key.to_owned(), value.trim().to_owned()))
        })
        .collect()
}
fn worker_pid(server: &Server) -> u32 {
    let parent = server.child.id();
    let children =
        std::fs::read_to_string(format!("/proc/{parent}/task/{parent}/children")).unwrap();
    let children: Vec<u32> = children
        .split_whitespace()
        .map(|pid| pid.parse().unwrap())
        .collect();
    assert_eq!(children.len(), 1, "exactly one application worker");
    children[0]
}

#[test]
fn actual_monitor_and_http_threads_have_separate_capability_sets() {
    let mut server = Server::start();
    let worker = worker_pid(&server);
    let allowed = (1_u64 << 2) | (1 << 19) | (1 << 21) | (1 << 38) | (1 << 39);
    for (pid, application) in [(server.child.id(), false), (worker, true)] {
        let status = status_fields(pid);
        for field in ["CapInh", "CapAmb"] {
            assert_eq!(
                u64::from_str_radix(&status[field], 16).unwrap(),
                0,
                "{pid} {field}"
            );
        }
        assert_eq!(status["NoNewPrivs"], "1");
        if application {
            for field in ["Uid", "Gid"] {
                assert!(
                    status[field]
                        .split_whitespace()
                        .all(|id| id.parse::<u32>().unwrap() != 0)
                );
            }
        }
        // Linux capabilities are per-thread: inspect every actual runtime and
        // transport thread rather than just the process leader.
        for entry in std::fs::read_dir(format!("/proc/{pid}/task")).unwrap() {
            let path = entry.unwrap().path().join("status");
            let thread = std::fs::read_to_string(path).unwrap();
            for field in ["CapPrm:", "CapEff:"] {
                let value = thread
                    .lines()
                    .find(|line| line.starts_with(field))
                    .unwrap()
                    .split_whitespace()
                    .nth(1)
                    .unwrap();
                let caps = u64::from_str_radix(value, 16).unwrap();
                assert_eq!(caps & !allowed, 0, "unexpected monitor capability");
                if application {
                    assert_eq!(caps, 0);
                } else if std::env::var_os("PROCINSH_REQUIRE_PERF").is_some() {
                    assert_eq!(
                        caps, allowed,
                        "dev_run must provide all inspection capabilities"
                    );
                }
            }
        }
    }
    if std::env::var_os("PROCINSH_REQUIRE_PERF").is_some() {
        let (_, _, body) = server.get("/api/processes");
        let processes: serde_json::Value = serde_json::from_str(&body).unwrap();
        let identity = &processes
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["identity"]["pid"] == worker)
            .unwrap()["identity"];
        let query = format!(
            "pid={worker}&start_time_ticks={}",
            identity["start_time_ticks"]
        );
        // The dumpable=0 HTTP worker cannot read these protected entries for
        // itself through another unprivileged process. The monitor can.
        for resource in ["maps", "environment", "auxv", "fds"] {
            assert_eq!(
                server.get(&format!("/api/processes/{resource}?{query}")).0,
                200
            );
        }
    }
    server.shutdown();
}

#[test]
fn worker_crash_reaps_parent_and_releases_listener() {
    let mut server = Server::start();
    let worker = worker_pid(&server);
    let _system = server.events("/api/system/events", "snapshot");
    assert_eq!(unsafe { libc::kill(worker as i32, libc::SIGKILL) }, 0);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = server.child.try_wait().unwrap() {
            assert!(!status.success());
            break;
        }
        assert!(
            Instant::now() < deadline,
            "monitor did not stop after worker crash"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(TcpStream::connect_timeout(&server.address, Duration::from_millis(200)).is_err());
}

#[test]
fn monitor_crash_stops_http_worker() {
    let mut server = Server::start();
    let worker = worker_pid(&server);
    server.child.kill().unwrap();
    server.child.wait().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while TcpStream::connect_timeout(&server.address, Duration::from_millis(100)).is_ok() {
        assert!(
            Instant::now() < deadline,
            "HTTP worker {worker} outlived its monitor"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn missing_capabilities_report_errors_without_stopping_http() {
    let mut server = Server::start_with_privileges(false);
    let parent = status_fields(server.child.id());
    assert_eq!(u64::from_str_radix(&parent["CapPrm"], 16).unwrap(), 0);
    let worker = worker_pid(&server);
    let (_, _, body) = server.get("/api/processes");
    let processes: serde_json::Value = serde_json::from_str(&body).unwrap();
    let identity = &processes
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["identity"]["pid"] == worker)
        .unwrap()["identity"];
    let (status, _, body) = server.get(&format!(
        "/api/processes/maps?pid={worker}&start_time_ticks={}",
        identity["start_time_ticks"]
    ));
    assert_eq!(
        status, 200,
        "maps retain their partial-result contract: {body}"
    );
    assert!(serde_json::from_str::<serde_json::Value>(&body).unwrap()["error"].is_string());
    assert_eq!(server.get("/api/config").0, 200);
    server.shutdown();
}
