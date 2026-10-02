"""Inside-container assertions for tests/container.Dockerfile; fails on unavailable sensors."""
import json
import os
from pathlib import Path
import socket
import subprocess
import struct
import sys
import threading
import time
import urllib.request
from sse import Stream


def fixture():
    # Keep resources open until the observer has discovered their identities.
    pipe = os.pipe()
    sockets = socket.socketpair()
    with open("/tmp/procinsh-container-activity", "w+b", buffering=0) as file:
        print(json.dumps({"pid": os.getpid(), "inode": os.fstat(file.fileno()).st_ino}), flush=True)
        sys.stdin.readline()
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            os.write(pipe[1], b"pipe")
            os.read(pipe[0], 4)
            sockets[0].send(b"socket")
            sockets[1].recv(6)
            os.lseek(file.fileno(), 0, os.SEEK_SET)
            os.write(file.fileno(), b"file")
            os.lseek(file.fileno(), 0, os.SEEK_SET)
            os.read(file.fileno(), 4)
            # Exercise scheduler activity as well as blocking I/O.
            until = time.monotonic() + .02
            while time.monotonic() < until:
                pass
        sys.stdin.readline()


if "--fixture" in sys.argv:
    fixture()
    sys.exit(0)

server = child = response = reader = None
dns = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
dns.bind(("127.0.0.1", 53))
dns.settimeout(.2)
dns_stop = threading.Event()
def answer_dns():
    while not dns_stop.is_set():
        try:
            query, sender = dns.recvfrom(4096)
        except socket.timeout:
            continue
        name = b"\x07fixture\x07example\x04test\x00"
        # Return one PTR answer for the controlled reverse lookup below.
        reply = query[:2] + struct.pack("!HHHHH", 0x8180, 1, 1, 0, 0) + query[12:]
        reply += b"\xc0\x0c" + struct.pack("!HHIH", 12, 1, 60, len(name)) + name
        dns.sendto(reply, sender)
dns_reader = threading.Thread(target=answer_dns, daemon=True)
dns_reader.start()
# These are per-container Docker configuration files, never host files.
Path("/etc/resolv.conf").write_text("nameserver 127.0.0.1\noptions timeout:1 attempts:1\n")
Path("/etc/nsswitch.conf").write_text("hosts: files dns\n")
try:
    assert str(Path("/proc/self/ns/pid").readlink()) != os.environ["HOST_PIDNS"]
    assert Path("/proc/self/ns/pid").readlink() == Path("/proc/1/ns/pid").readlink()
    server = subprocess.Popen(["./scripts/dev_run.sh", "--listen", "0.0.0.0:9090", "--allow-non-loopback"])
    base = "http://127.0.0.1:9090"
    for _ in range(100):
        try:
            urllib.request.urlopen(base + "/api/processes", timeout=1).close()
            break
        except OSError:
            if server.poll() is not None:
                raise RuntimeError("server exited")
            time.sleep(.1)
    else:
        raise AssertionError("HTTP server did not start")
    child = subprocess.Popen([sys.executable, __file__, "--fixture"], stdin=subprocess.PIPE,
                             stdout=subprocess.PIPE, text=True)
    ready = json.loads(child.stdout.readline())
    local_pid = ready["pid"]
    stat = Path(f"/proc/{local_pid}/stat").read_text().rsplit(")", 1)[1].split()
    identity = {"pid": local_pid, "start_time_ticks": int(stat[19])}
    response = Stream(base + "/api/system/events", timeout=30)
    frames = []
    snapshots = []
    def consume():
        try:
            for line in response:
                if line.startswith(b"data: "):
                    value = json.loads(line[6:])
                    if "processes" in value:
                        snapshots.append(value)
                    elif "cpu" in value:
                        frames.append(value)
        except (OSError, ValueError):
            pass
    reader = threading.Thread(target=consume, daemon=True)
    reader.start()
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        if frames:
            status = frames[-1]["status"]
            assert all(status[s]["state"] not in ("unavailable", "error") for s in ("cpu", "ipc", "files")), status
        if snapshots and any(p["identity"] == identity for p in snapshots[-1]["processes"]):
            break
        time.sleep(.1)
    else:
        raise AssertionError("procfs did not discover container-local fixture")
    child.stdin.write("go\n")
    child.stdin.flush()
    deadline = time.monotonic() + 15
    while time.monotonic() < deadline:
        matching = {sensor: [event for frame in list(frames) for event in frame[sensor]
                             if event["process_id"] == identity] for sensor in ("cpu", "ipc", "files")}
        if (any(e["runtime_ns"] > 0 for e in matching["cpu"])
                and {e["resource"]["kind"] for e in matching["ipc"]} >= {"pipe", "socket"}
                and any(int(e["file"]["inode"]) == ready["inode"] and e["bytes"] > 0 for e in matching["files"])):
            break
        time.sleep(.1)
    else:
        raise AssertionError(("BPF and procfs identity mismatch", identity, {s: len(events) for s, events in matching.items()}, [e for f in frames for e in f["files"]][:3], frames[-1:]))
    # Host-only process names and paths supplied by the outer integration test.
    assert not any(p["name"] == "outside-pidns" for snap in snapshots for p in snap["processes"])
    assert not any(e.get("path") == "/tmp/procinsh-outside-pidns" for frame in frames for e in frame["files"])
    # User names are read from the container's passwd file, without NSS.
    fixture_process = next(p for p in snapshots[-1]["processes"] if p["identity"] == identity)
    assert fixture_process["username"] == "root", fixture_process
    # A loopback TCP connection invokes the static binary's reverse resolver.
    connection = socket.create_connection(("127.0.0.1", 9090))
    try:
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if any((e.get("socket") or {}).get("remote_hostname") == "localhost"
                   for snap in list(snapshots) for e in snap.get("fd_relations", [])):
                break
            time.sleep(.1)
        else:
            raise AssertionError(("reverse DNS did not resolve localhost", snapshots[-1:]))
    finally:
        connection.close()
    remote = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    try:
        remote.connect(("192.0.2.1", 54321))
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            if any((e.get("socket") or {}).get("remote_hostname") == "fixture.example.test"
                   for snap in list(snapshots) for e in snap.get("fd_relations", [])):
                break
            time.sleep(.1)
        else:
            raise AssertionError("static glibc DNS PTR lookup failed")
    finally:
        remote.close()
    print("Container passed: procfs/CPU/pipe/socket/file PID identities, namespace isolation, passwd and reverse DNS", flush=True)
finally:
    dns_stop.set()
    dns_reader.join(timeout=2)
    dns.close()
    if response:
        response.close(reader)
    for process in (child, server):
        if process and process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
