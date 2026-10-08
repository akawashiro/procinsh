"""Run after cargo build: python3 tests/listen-policy.py.

Set PROCINSH_BINARY=./scripts/dev_run.sh to use the local capability launcher.
"""
from contextlib import contextmanager
import http.client
import os
import re
import signal
import socket
import subprocess
import tempfile
import time
from urllib.parse import urlsplit

COMMAND = [os.environ.get("PROCINSH_BINARY", "target/debug/procinsh")]


def environment():
    env = dict(os.environ)
    # Startup detection requires info output regardless of the caller's filters.
    env["RUST_LOG"] = "info"
    env["RUST_LOG_STYLE"] = "never"
    return env


@contextmanager
def running_server(addresses, opt_in=False, shutdown_signal=signal.SIGTERM):
    with tempfile.TemporaryFile() as stderr:
        args = list(COMMAND)
        for address in addresses:
            args.extend(["--listen", address])
        if opt_in:
            args.append("--allow-non-loopback")
        process = subprocess.Popen(args, stdout=subprocess.DEVNULL,
                                   stderr=stderr, env=environment())
        try:
            deadline = time.monotonic() + 10
            while True:
                stderr.seek(0)
                output = stderr.read().decode()
                assert process.poll() is None, output
                bound = re.findall(r"listening on http://(\S+)", output)
                if len(bound) == len(addresses):
                    break
                assert time.monotonic() < deadline, output
                time.sleep(0.05)
            yield bound
            process.send_signal(shutdown_signal)
            assert process.wait(timeout=10) == 0
        finally:
            if process.poll() is None:
                process.kill()
                process.wait()


def request(address, host):
    parsed = urlsplit(f"http://{address}")
    connection = http.client.HTTPConnection(parsed.hostname, parsed.port, timeout=5)
    try:
        connection.request("GET", "/api/config", headers={"Host": host})
        response = connection.getresponse()
        response.read()
        return response.status
    finally:
        connection.close()


def check_listen_policy():
    help_result = subprocess.run(
        [*COMMAND, "--help"], capture_output=True, text=True, timeout=10,
    )
    assert help_result.returncode == 0
    for expected in ("127.0.0.1:8080", "--allow-non-loopback", "authentication", "TLS"):
        assert expected in help_result.stdout, help_result.stdout

    # Unassigned LAN addresses must fail the policy check, not the bind syscall.
    for address in ("0.0.0.0:0", "[::]:0", "192.168.1.10:0", "[fd00::1234]:0"):
        result = subprocess.run(
            [*COMMAND, "--listen", address], env=environment(),
            capture_output=True, text=True, timeout=10,
        )
        assert result.returncode != 0
        assert result.stdout == ""
        for expected in ("refusing to listen", address, "--allow-non-loopback",
                         "process memory", "environment variables", "authentication", "TLS"):
            assert expected in result.stderr, result.stderr
        assert "could not bind HTTP listener" not in result.stderr, result.stderr

    # An occupied wildcard port also proves rejection precedes binding.
    with socket.socket() as occupied:
        occupied.bind(("0.0.0.0", 0))
        occupied.listen()
        result = subprocess.run(
            [*COMMAND, "--listen", f"0.0.0.0:{occupied.getsockname()[1]}"],
            env=environment(), capture_output=True, text=True, timeout=10,
        )
        assert result.returncode != 0
        assert "refusing to listen" in result.stderr, result.stderr
        assert "could not bind HTTP listener" not in result.stderr, result.stderr

    for address, opt_in in (("127.0.0.1:0", False), ("[::1]:0", False),
                            ("0.0.0.0:0", True), ("[::]:0", True)):
        with running_server([address], opt_in):
            pass

    # Validate every address before binding, regardless of its position.
    for addresses in (("127.0.0.1:0", "192.168.1.10:0"),
                      ("192.168.1.10:0", "127.0.0.1:0")):
        result = subprocess.run(
            [*COMMAND, "--listen", addresses[0], "--listen", addresses[1]],
            env=environment(), capture_output=True, text=True, timeout=10,
        )
        assert result.returncode != 0
        assert "refusing to listen" in result.stderr, result.stderr
        assert "could not bind HTTP listener" not in result.stderr, result.stderr
        assert "listening on http://" not in result.stderr, result.stderr

    # A bind failure must not leave a partially running server.
    with socket.socket() as occupied:
        occupied.bind(("127.0.0.1", 0))
        occupied.listen()
        result = subprocess.run(
            [*COMMAND, "--listen", "127.0.0.1:0", "--listen",
             f"127.0.0.1:{occupied.getsockname()[1]}"],
            env=environment(), capture_output=True, text=True, timeout=10,
        )
        assert result.returncode != 0
        assert "could not bind HTTP listener" in result.stderr, result.stderr
        assert "listening on http://" not in result.stderr, result.stderr

    # Explicit addresses replace the default and retain separate Host guards.
    with running_server(["127.0.0.1:0", "127.0.0.2:0", "[::1]:0"],
                        shutdown_signal=signal.SIGINT) as bound:
        assert [urlsplit(f"http://{a}").hostname for a in bound] == [
            "127.0.0.1", "127.0.0.2", "::1",
        ], bound
        for address in bound:
            port = urlsplit(f"http://{address}").port
            assert request(address, address) == 200, address
            assert request(address, f"localhost:{port}") == 200, address
            other_host = "127.0.0.2" if address == bound[0] else "127.0.0.1"
            assert request(address, f"{other_host}:{port}") == 403, address
            assert request(address, f"evil.test:{port}") == 403, address

check_listen_policy()
print("Listen policy checks passed.")
