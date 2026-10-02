"""Run after cargo build: python3 tests/listen-policy.py.

Set PROCINSH_BINARY=./scripts/dev_run.sh to use the local capability launcher.
"""
import os
import socket
import subprocess
import tempfile
import time

COMMAND = [os.environ.get("PROCINSH_BINARY", "target/debug/procinsh")]


def environment():
    env = dict(os.environ)
    # Startup detection requires info output regardless of the caller's filters.
    env["RUST_LOG"] = "info"
    env["RUST_LOG_STYLE"] = "never"
    return env


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
        with tempfile.TemporaryFile() as stderr:
            args = [*COMMAND, "--listen", address]
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
                    if "listening on http://" in output:
                        break
                    assert time.monotonic() < deadline, output
                    time.sleep(0.05)
                process.terminate()
                assert process.wait(timeout=10) == 0
            finally:
                if process.poll() is None:
                    process.kill()
                    process.wait()


check_listen_policy()
print("Listen policy checks passed.")
