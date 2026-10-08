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
}
impl Server {
    fn start() -> Self {
        let mut child = Command::new(
            std::env::var_os("PROCINSH_BINARY")
                .unwrap_or_else(|| env!("CARGO_BIN_EXE_procinsh").into()),
        )
        .args(["--listen", "127.0.0.1:0"])
        .env("RUST_LOG", "info")
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
        let stderr = child.stderr.take().unwrap();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                if let Some(address) = line.split("listening on http://").nth(1) {
                    let _ = tx.send(address.split_whitespace().next().unwrap().to_owned());
                }
            }
        });
        // Install the cleanup guard before waiting for startup.
        let mut server = Self {
            child,
            address: "127.0.0.1:0".parse().unwrap(),
        };
        server.address = rx
            .recv_timeout(Duration::from_secs(10))
            .expect("server startup")
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
    for path in [
        "/",
        "/list",
        "/process/1",
        "/space",
        "/list/app.js",
        "/process/app.js",
        "/space/app.js",
        "/space/model.js",
        "/shared/api.js",
        "/shared/display.js",
        "/shared/dom.js",
        "/shared/navigation.js",
        "/list/style.css",
        "/process/style.css",
        "/space/style.css",
        "/shared/style.css",
        "/vendor/three.module.js",
        "/api/config",
    ] {
        let (status, headers, body) = server.get(path);
        assert_eq!(status, 200, "{path}");
        assert!(headers.contains("cache-control: no-store"));
        assert!(!body.is_empty());
        if path.ends_with(".js") {
            assert!(headers.contains("text/javascript; charset=utf-8"));
        } else if path.ends_with(".css") {
            assert!(headers.contains("text/css; charset=utf-8"));
        }
    }
    let list = server.get("/list").2;
    let process = server.get("/process/1").2;
    assert!(list.contains("id=\"explorer\""));
    assert!(!list.contains("id=\"inspector\""));
    assert!(process.contains("id=\"inspector\""));
    assert!(!process.contains("id=\"explorer\""));
    assert_eq!(server.get("/").2, list);
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
    let mut process = server.events(&format!("/api/processes/events?{query}"), "observation");
    let mut system = server.events("/api/system/events", "snapshot");
    server.shutdown();
    // Open streams must end so graceful shutdown can finish.
    process.read_to_end(&mut Vec::new()).unwrap();
    system.read_to_end(&mut Vec::new()).unwrap();
}
