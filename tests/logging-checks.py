"""Run after cargo build: python3 tests/logging-checks.py (no sudo required)."""
import os
import json
import re
import socket
import subprocess
import tempfile
import time
import urllib.request

BINARY = "target/debug/procinsh"


def environment(level):
    env = dict(os.environ)
    env.pop("RUST_LOG", None)
    env["RUST_LOG_STYLE"] = "never"
    if level is not None:
        env["RUST_LOG"] = level
    return env


def check_running(level):
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
        process = subprocess.Popen(
            [BINARY, "--listen", f"127.0.0.1:{port}"],
            stdout=stdout, stderr=stderr, env=environment(level),
        )
        try:
            deadline = time.monotonic() + 10
            while True:
                assert process.poll() is None, "server exited before ready"
                try:
                    with urllib.request.urlopen(
                        f"http://127.0.0.1:{port}/api/config?token=PRIVATE_SENTINEL", timeout=1
                    ) as response:
                        assert response.status == 200
                    break
                except OSError:
                    if time.monotonic() >= deadline:
                        raise
                    time.sleep(0.05)
            with urllib.request.urlopen(f"http://127.0.0.1:{port}/api/processes", timeout=5) as response:
                processes = json.load(response)
            identity = next(item["identity"] for item in processes if item["identity"]["pid"] == process.pid)
            paths = [f'/api/processes/events?pid={identity["pid"]}&start_time_ticks={identity["start_time_ticks"]}']
            if level == "procinsh=debug":
                paths.append("/api/system/events")
            for path in paths:
                with urllib.request.urlopen(f"http://127.0.0.1:{port}{path}", timeout=5) as response:
                    while response.readline().strip():
                        pass
            process.terminate()
            assert process.wait(timeout=10) == 0
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()
        stdout.seek(0)
        stderr.seek(0)
        assert stdout.read() == b""
        output = stderr.read().decode()
        assert "PRIVATE_SENTINEL" not in output
        assert "SPACE" not in output
        assert ("SSE /api/processes/events event=observation" in output) == (level == "procinsh=debug")
        if level == "off":
            assert output == "", output
        else:
            assert re.search(r"\[.*INFO\s+src/main\.rs:[1-9][0-9]*\]", output), output
            assert f"http://127.0.0.1:{port}" in output
            assert "SIGTERM" in output and "procinsh stopped" in output
            assert ('HTTP GET "/api/config" status=200' in output) == (level == "procinsh=debug")
            if level == "procinsh=debug":
                assert "SSE /api/system/events event=topology" in output, output
                assert re.search(r"\[.*DEBUG\s+src/server/mod\.rs:[1-9][0-9]*\] HTTP GET", output), output


for level in (None, "procinsh=debug", "off"):
    check_running(level)

with socket.socket() as occupied:
    occupied.bind(("127.0.0.1", 0))
    occupied.listen()
    result = subprocess.run(
        [BINARY, "--listen", f"127.0.0.1:{occupied.getsockname()[1]}"],
        env=environment(None), capture_output=True, text=True, timeout=10,
    )
    assert result.returncode != 0
    assert result.stdout == ""
    assert result.stderr.count("could not bind HTTP listener") == 1, result.stderr
    assert re.search(r"ERROR src/main\.rs:[1-9][0-9]*\]", result.stderr), result.stderr

print("Logging checks passed: default info, debug HTTP, off, stderr, SIGTERM, bind failure, query omission.")
