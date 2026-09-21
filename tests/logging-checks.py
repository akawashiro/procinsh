"""Run after cargo build: python3 tests/logging-checks.py (no sudo required)."""
import os
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
        if level == "off":
            assert output == "", output
        else:
            assert re.search(r"\[.*INFO\s+procinsh\]", output), output
            assert f"http://127.0.0.1:{port}" in output
            assert "SIGTERM" in output and "procinsh stopped" in output
            assert ('HTTP GET "/api/config" status=200' in output) == (level == "procinsh=debug")


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
    assert "ERROR procinsh" in result.stderr

print("Logging checks passed: default info, debug HTTP, off, stderr, SIGTERM, bind failure, query omission.")
