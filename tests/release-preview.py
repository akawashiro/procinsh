#!/usr/bin/env python3
"""Exercise release deployment without network, systemd, or privileges."""

import fcntl
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


SCRIPT = Path(__file__).resolve().parents[1] / "scripts/release_preview.sh"
MOCK = r'''
import fcntl
import json
import os
from pathlib import Path
import sys

name = Path(sys.argv[0]).name
args = sys.argv[1:]
state = Path(os.environ["PROCINSH_RELEASE_PREVIEW_STATE"])
# Every external deployment step must run while the update lock is held.
with (state / "update.lock").open("w") as lock:
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        pass
    else:
        raise AssertionError("update lock was released during deployment")
with (state / "calls.jsonl").open("a") as log:
    log.write(json.dumps([name, *args]) + "\n")
if name == "cargo":
    assert args[0] == "install"
    root = Path(args[args.index("--root") + 1])
    assert root == state / "install"
    if "--list" in args:
        print("procinsh v" + (root / "version").read_text() + ":")
        print("    procinsh")
    else:
        assert args[:3] == ["install", "procinsh", "--locked"]
        assert args[args.index("--registry") + 1] == "crates-io"
        assert "--force" not in args
        assert os.environ["CARGO_TARGET_DIR"] == str(state / "target")
        if os.environ.get("FAIL_INSTALL"):
            sys.exit(1)
        version = (state / "latest").read_text()
        (root / "bin").mkdir(parents=True, exist_ok=True)
        (root / "bin/procinsh").write_text("binary for " + version)
        (root / "version").write_text(version)
elif name == "sudo":
    assert args == ["-n", "setcap",
                    "cap_sys_ptrace,cap_bpf,cap_perfmon,cap_dac_read_search=ep",
                    str(state / "candidate")]
    assert (state / "candidate").stat().st_mode & 0o777 == 0o755
    if os.environ.get("FAIL_SETCAP"):
        sys.exit(1)
elif name == "systemctl":
    assert args[0] == "--user"
    assert args[-1] == "procinsh-release-preview.service"
    if args[1] == "restart" and os.environ.get("FAIL_RESTART"):
        sys.exit(1)
    if args[1] == "is-active" and os.environ.get("FAIL_ACTIVE"):
        sys.exit(1)
elif name == "curl":
    assert args == ["--noproxy", "*", "--fail", "--silent", "--max-time", "1",
                    "http://127.0.0.1:9091/"]
    if os.environ.get("FAIL_HTTP"):
        sys.exit(1)
elif name == "sleep":
    assert args == ["0.5"]
else:
    raise AssertionError(name)
'''


class ReleasePreviewTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.state = Path(self.tmp.name) / "state"
        self.state.mkdir()
        self.bin = Path(self.tmp.name) / "bin"
        self.bin.mkdir()
        for name in ("cargo", "sudo", "systemctl", "curl", "sleep"):
            command = self.bin / name
            command.write_text(f"#!{sys.executable}\n" + MOCK)
            command.chmod(0o755)
        self.env = os.environ.copy()
        for key in ("FAIL_INSTALL", "FAIL_SETCAP", "FAIL_RESTART", "FAIL_ACTIVE", "FAIL_HTTP"):
            self.env.pop(key, None)
        self.env.update(
            PATH=f"{self.bin}:/usr/bin:/bin",
            PROCINSH_RELEASE_PREVIEW_STATE=str(self.state),
        )
        self.latest("0.1.8")

    def latest(self, version):
        (self.state / "latest").write_text(version)

    def update(self, expected=0, **failures):
        result = subprocess.run(
            ["/bin/bash", str(SCRIPT), "update"],
            env={**self.env, **failures},
            capture_output=True,
            text=True,
            timeout=15,
        )
        self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
        return result

    def calls(self):
        log = self.state / "calls.jsonl"
        return [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []

    def clear_calls(self):
        (self.state / "calls.jsonl").unlink(missing_ok=True)

    def assert_running(self, version):
        self.assertEqual((self.state / "current.version").read_text(), version + "\n")
        self.assertEqual((self.state / "procinsh").read_text(), "binary for " + version)
        self.assertFalse((self.state / "candidate.version").exists())

    def test_first_install_and_unchanged_version(self):
        self.update()
        self.assert_running("0.1.8")
        self.assertIn(["systemctl", "--user", "restart", "procinsh-release-preview.service"], self.calls())
        self.assertTrue(any(call[0] == "curl" for call in self.calls()))
        self.clear_calls()
        result = self.update()
        self.assertIn("Already running release 0.1.8", result.stdout)
        self.assert_running("0.1.8")
        self.assertEqual([call[0] for call in self.calls()], ["cargo", "cargo"])

    def test_new_release(self):
        self.update()
        previous_inode = (self.state / "procinsh").stat().st_ino
        self.latest("0.1.9")
        self.update()
        self.assert_running("0.1.9")
        self.assertEqual((self.state / "previous").stat().st_ino, previous_inode)
        self.assertEqual((self.state / "previous.version").read_text(), "0.1.8\n")

    def test_pre_activation_failures_keep_running_version(self):
        self.update()
        self.latest("0.1.9")
        for failure in ("FAIL_INSTALL", "FAIL_SETCAP"):
            with self.subTest(failure=failure):
                # Stale candidates must never be activated after a failed update.
                (self.state / "candidate").write_text("stale")
                (self.state / "candidate.version").write_text("stale")
                self.clear_calls()
                self.update(expected=1, **{failure: "1"})
                self.assert_running("0.1.8")
                self.assertFalse(any(call[0] == "systemctl" for call in self.calls()))
        self.update()
        self.assert_running("0.1.9")

    def test_failed_activation_rolls_back_and_retries(self):
        self.update()
        self.latest("0.1.9")
        for failure in ("FAIL_RESTART", "FAIL_ACTIVE", "FAIL_HTTP"):
            with self.subTest(failure=failure):
                previous_inode = (self.state / "procinsh").stat().st_ino
                self.clear_calls()
                self.update(expected=1, **{failure: "1"})
                self.assert_running("0.1.8")
                self.assertEqual((self.state / "procinsh").stat().st_ino, previous_inode)
                self.assertIn(["systemctl", "--user", "stop", "procinsh-release-preview.service"], self.calls())
                self.assertIn(["systemctl", "--user", "start", "procinsh-release-preview.service"], self.calls())
                self.assertEqual((self.state / "install/version").read_text(), "0.1.9")
        self.update()
        self.assert_running("0.1.9")

    def test_failed_first_start_stops_and_removes_active_binary(self):
        self.update(expected=1, FAIL_HTTP="1")
        self.assertFalse((self.state / "procinsh").exists())
        self.assertFalse((self.state / "current.version").exists())
        self.assertIn(["systemctl", "--user", "stop", "procinsh-release-preview.service"], self.calls())
        self.assertFalse(any(call[:3] == ["systemctl", "--user", "start"] for call in self.calls()))
        self.update()
        self.assert_running("0.1.8")

    def test_lock_skips_concurrent_update(self):
        with (self.state / "update.lock").open("w") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.update()
            self.assertEqual(self.calls(), [])
        self.update()
        self.assert_running("0.1.8")


if __name__ == "__main__":
    unittest.main()
