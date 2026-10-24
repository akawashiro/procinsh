//! Exercise the installed interface: start the binary and communicate over HTTP.
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpStream},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};

// Own the observed fixture so failed HTTP assertions still reap it.
struct Target(Child);
impl Drop for Target {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

struct DirectoryGuard(std::path::PathBuf);
impl Drop for DirectoryGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

struct Server {
    child: Child,
    address: SocketAddr,
    second_address: SocketAddr,
}
impl Server {
    fn start() -> Self {
        let command = Command::new(
            std::env::var_os("PROCINSH_BINARY")
                .unwrap_or_else(|| env!("CARGO_BIN_EXE_procinsh").into()),
        );
        Self::start_command(command)
    }

    fn start_command(mut command: Command) -> Self {
        let mut child = command
            .args(["--listen", "127.0.0.1:0", "--listen", "[::1]:0"])
            .env("RUST_LOG", "info")
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
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
    for endpoint in [
        "observation",
        "threads",
        "maps",
        "environment",
        "auxv",
        "fds",
    ] {
        let (status, _, body) = server.get(&format!("/api/processes/{endpoint}?{query}"));
        assert_eq!(status, 200, "{endpoint}: {body}");
        serde_json::from_str::<serde_json::Value>(&body).unwrap();
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

fn helper_pid(server: &Server) -> u32 {
    let children =
        std::fs::read_to_string(format!("/proc/{0}/task/{0}/children", server.child.id())).unwrap();
    let children: Vec<u32> = children
        .split_whitespace()
        .map(|p| p.parse().unwrap())
        .collect();
    assert_eq!(children.len(), 1, "one separately executed helper");
    let executable = std::fs::read_link(format!("/proc/{}/exe", children[0])).unwrap();
    assert_eq!(
        executable.file_name().unwrap(),
        "procinsh-privileged-helper"
    );
    children[0]
}

fn check_thread_capabilities(pid: u32, expected: u64) {
    for thread in std::fs::read_dir(format!("/proc/{pid}/task")).unwrap() {
        let status = std::fs::read_to_string(thread.unwrap().path().join("status")).unwrap();
        for field in ["CapPrm:", "CapEff:", "CapInh:", "CapAmb:"] {
            let value = status
                .lines()
                .find(|line| line.starts_with(field))
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap();
            let wanted = if matches!(field, "CapInh:" | "CapAmb:") {
                0
            } else {
                expected
            };
            assert_eq!(
                u64::from_str_radix(value, 16).unwrap(),
                wanted,
                "{pid} {field}"
            );
        }
        assert!(
            status
                .lines()
                .any(|line| line.starts_with("NoNewPrivs:") && line.ends_with('1'))
        );
    }
}

fn check_process_identity(pid: u32, uid: u32, gid: u32) {
    for thread in std::fs::read_dir(format!("/proc/{pid}/task")).unwrap() {
        let status = std::fs::read_to_string(thread.unwrap().path().join("status")).unwrap();
        for (field, expected) in [("Uid:", uid), ("Gid:", gid)] {
            for id in status
                .lines()
                .find(|line| line.starts_with(field))
                .unwrap()
                .split_whitespace()
                .skip(1)
            {
                assert_eq!(id.parse::<u32>().unwrap(), expected, "{pid} {field}");
            }
        }
    }
}

#[test]
fn ordinary_user_identity_is_preserved() {
    use std::os::unix::process::CommandExt;
    let mut command = Command::new(env!("CARGO_BIN_EXE_procinsh"));
    command.uid(65534).gid(65534);
    let mut server = Server::start_command(command);
    let helper = helper_pid(&server);
    check_process_identity(server.child.id(), 65534, 65534);
    check_process_identity(helper, 65534, 65534);
    check_thread_capabilities(server.child.id(), 0);
    check_thread_capabilities(
        helper,
        (1 << 2) | (1 << 19) | (1 << 21) | (1 << 38) | (1 << 39),
    );
    assert_eq!(server.get("/api/processes").0, 200);
    server.shutdown();
}

#[test]
fn capabilities_and_resource_ownership_stay_in_separate_helper() {
    let mut command = Command::new(env!("CARGO_BIN_EXE_procinsh"));
    command
        .env("SUDO_UID", "invalid")
        .env("SUDO_GID", "invalid");
    let mut server = Server::start_command(command);
    let helper = helper_pid(&server);
    let stream = server.events("/api/system/events", "snapshot");
    for pid in [server.child.id(), helper] {
        check_process_identity(pid, unsafe { libc::getuid() }, unsafe { libc::getgid() });
    }
    check_thread_capabilities(server.child.id(), 0);
    check_thread_capabilities(
        helper,
        (1 << 2) | (1 << 19) | (1 << 21) | (1 << 38) | (1 << 39),
    );
    for descriptor in std::fs::read_dir(format!("/proc/{}/fd", server.child.id())).unwrap() {
        let target = std::fs::read_link(descriptor.unwrap().path())
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert!(
            !target.starts_with("/proc/")
                && !target.contains("perf_event")
                && !target.contains("bpf"),
            "privileged descriptor in HTTP process: {target}"
        );
    }
    let environment = std::fs::read(format!("/proc/{helper}/environ")).unwrap();
    assert!(
        environment
            .split(|b| *b == 0)
            .filter(|v| !v.is_empty())
            .all(|v| v.starts_with(b"RUST_LOG=")),
        "helper environment must be explicitly whitelisted"
    );
    let listeners: std::collections::HashSet<String> = ["tcp", "tcp6"]
        .into_iter()
        .flat_map(|table| {
            std::fs::read_to_string(format!("/proc/{helper}/net/{table}"))
                .unwrap()
                .lines()
                .skip(1)
                .filter_map(|line| {
                    let columns: Vec<_> = line.split_whitespace().collect();
                    (columns.get(3) == Some(&"0A")).then(|| format!("socket:[{}]", columns[9]))
                })
                .collect::<Vec<_>>()
        })
        .collect();
    for fd in std::fs::read_dir(format!("/proc/{helper}/fd")).unwrap() {
        let Ok(target) = std::fs::read_link(fd.unwrap().path()) else {
            continue;
        };
        let target = target.to_string_lossy().into_owned();
        assert!(
            !listeners.contains(&target),
            "helper must own no TCP listener"
        );
    }
    drop(stream);
    server.shutdown();
    assert!(
        !std::path::Path::new(&format!("/proc/{helper}")).exists(),
        "helper must be reaped on normal shutdown"
    );
}

#[test]
fn helper_crash_keeps_http_responsive_with_unavailable_inspection() {
    let mut server = Server::start();
    let helper = helper_pid(&server);
    let target = Target(
        Command::new("tests/targets/bin/sleeping")
            .arg("--allow-inspector")
            .stdout(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let processes: serde_json::Value =
        serde_json::from_str(&server.get("/api/processes").2).unwrap();
    let id = &processes
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["identity"]["pid"] == target.0.id())
        .unwrap()["identity"];
    let query = format!(
        "pid={}&start_time_ticks={}",
        id["pid"], id["start_time_ticks"]
    );
    let stream = server.events(&format!("/api/processes/events?{query}"), "observation");
    assert_eq!(unsafe { libc::kill(helper as i32, libc::SIGKILL) }, 0);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let status = server.get("/api/processes").0;
        if status == 503 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "helper crash must report unavailable; got {status}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let status = std::fs::read_to_string(format!("/proc/{}/status", target.0.id())).unwrap();
    assert!(
        status
            .lines()
            .any(|line| line.starts_with("TracerPid:")
                && line.split_whitespace().nth(1) == Some("0")),
        "target retained a tracer after helper death"
    );
    assert!(
        !status.lines().any(|line| line.starts_with("State:")
            && (line.contains("T (stopped)") || line.contains("t (tracing stop)"))),
        "target remained stopped"
    );
    drop(stream);
    assert_eq!(server.get("/api/config").0, 200);
    server.shutdown();
}

#[test]
fn parent_crash_releases_helper() {
    let mut server = Server::start();
    let helper = helper_pid(&server);
    let stream = server.events("/api/system/events", "snapshot");
    server.child.kill().unwrap();
    server.child.wait().unwrap();
    drop(stream);
    let deadline = Instant::now() + Duration::from_secs(10);
    while let Ok(status) = std::fs::read_to_string(format!("/proc/{helper}/status")) {
        if status
            .lines()
            .any(|line| line.starts_with("State:") && line.contains('Z'))
        {
            break;
        }
        assert!(Instant::now() < deadline, "helper survived parent crash");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn sibling_discovery_and_missing_helper_degrade_cleanly() {
    let helper = std::path::Path::new(env!("CARGO_BIN_EXE_procinsh-privileged-helper"));
    use std::os::unix::fs::MetadataExt;
    assert_ne!(
        std::fs::metadata(helper).unwrap().uid(),
        0,
        "test setup must cover a user-owned helper executable"
    );
    let main = helper.with_file_name("procinsh");
    let mut server = Server::start_command(Command::new(main));
    helper_pid(&server);
    assert_eq!(server.get("/api/processes").0, 200);
    server.shutdown();

    let path = std::env::temp_dir().join(format!(
        "procinsh-missing-helper-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir(&path).unwrap();
    let directory = DirectoryGuard(path);
    let isolated_main = directory.0.join("procinsh");
    std::fs::copy(env!("CARGO_BIN_EXE_procinsh"), &isolated_main).unwrap();
    let mut server = Server::start_command(Command::new(isolated_main));
    check_thread_capabilities(server.child.id(), 0);
    assert_eq!(server.get("/api/config").0, 200);
    assert_eq!(server.get("/api/processes").0, 503);
    server.shutdown();
}
