"""Exercise preview deployment decisions without touching host services."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[1]
SCRIPTS = ROOT / "users/akawashiro/scripts"
STUB = """#!/usr/bin/python3
import json, os, sys
from pathlib import Path
name = Path(sys.argv[0]).name
args = sys.argv[1:]
with open(os.environ['PREVIEW_TEST_LOG'], 'a') as log:
    log.write(json.dumps([name, *args]) + '\\n')
if name == 'curl' and args[-1].endswith('/api/processes'):
    sys.exit(22 if os.environ['PREVIEW_TEST_API'] == 'failed' else 0)
if name == 'git' and 'rev-parse' in args:
    print('new')
if name == 'cargo' and '--list' in args:
    print('procinsh v1.0.0:')
"""


class PreviewUpdateTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        self.state = self.root / "state"
        self.state.mkdir()
        self.bin = self.root / "bin"
        self.bin.mkdir()
        for command in ["systemctl", "curl", "sleep", "git", "npm", "cargo", "sudo"]:
            path = self.bin / command
            path.write_text(STUB)
            path.chmod(0o755)
        self.log = self.root / "commands.jsonl"
        self.env = {
            **os.environ,
            "PATH": f"{self.bin}:{os.environ['PATH']}",
            "PROCINSH_PREVIEW_STATE": str(self.state),
            "PROCINSH_PREVIEW_SOURCE": str(ROOT),
            "PROCINSH_RELEASE_PREVIEW_STATE": str(self.state),
            "PREVIEW_TEST_LOG": str(self.log),
            "PREVIEW_TEST_API": "healthy",
        }
        (self.state / "preview_bind.sh").write_text("echo 100.64.0.1\n")
        (self.state / "procinsh").write_text("old executable\n")
        for directory in [self.state / "worktree/target/debug", self.state / "install/bin"]:
            directory.mkdir(parents=True)
            for name in ["procinsh", "procinsh-privileged-helper"]:
                path = directory / name
                path.write_text("#!/bin/sh\nexit 0\n")
                path.chmod(0o755)

    def run_script(self, name, action):
        return subprocess.run(
            ["bash", str(SCRIPTS / name), action], env=self.env,
            text=True, capture_output=True, timeout=10,
        )

    def commands(self):
        return [json.loads(line) for line in self.log.read_text().splitlines()]

    def test_main_activation_checks_helper_and_rolls_back(self):
        for healthy in [True, False]:
            with self.subTest(healthy=healthy):
                (self.state / "procinsh").write_text("old executable\n")
                (self.state / "current.commit").write_text("old\n")
                (self.state / "candidate.commit").write_text("new\n")
                (self.state / "candidate").write_text("new executable\n")
                self.env["PREVIEW_TEST_API"] = "healthy" if healthy else "failed"
                result = self.run_script("main_preview.sh", "activate")
                self.assertEqual(result.returncode, 0 if healthy else 1, result.stderr)
                self.assertEqual((self.state / "current.commit").read_text().strip(),
                                 "new" if healthy else "old")
                if not healthy:
                    self.assertIn("Restored old", result.stderr)
                    self.assertEqual((self.state / "procinsh").read_text(), "old executable\n")

    def test_release_activation_checks_helper_and_rolls_back(self):
        for healthy in [True, False]:
            with self.subTest(healthy=healthy):
                (self.state / "procinsh").write_text("old executable\n")
                (self.state / "current.version").write_text("0.9.0\n")
                self.env["PREVIEW_TEST_API"] = "healthy" if healthy else "failed"
                result = self.run_script("release_preview.sh", "update")
                self.assertEqual(result.returncode, 0 if healthy else 1, result.stderr)
                self.assertEqual((self.state / "current.version").read_text().strip(),
                                 "1.0.0" if healthy else "0.9.0")
                if not healthy:
                    self.assertIn("Restored release 0.9.0", result.stderr)
                    self.assertEqual((self.state / "procinsh").read_text(), "old executable\n")

    def test_same_main_commit_is_rebuilt_when_helper_is_unavailable(self):
        (self.state / "current.commit").write_text("new\n")
        result = self.run_script("main_preview.sh", "build")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.state / "candidate.commit").exists())
        self.env["PREVIEW_TEST_API"] = "failed"
        result = self.run_script("main_preview.sh", "build")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.state / "candidate.commit").read_text(), "new\n")
        pairs = list(self.state.glob("pair.*"))
        self.assertEqual(len(pairs), 1)
        self.assertTrue((pairs[0] / "procinsh").is_file())
        self.assertTrue((pairs[0] / "procinsh-privileged-helper").is_file())

    def test_same_release_is_redeployed_when_helper_is_unavailable(self):
        (self.state / "current.version").write_text("1.0.0\n")
        result = self.run_script("release_preview.sh", "update")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(any(command[0] == "systemctl" for command in self.commands()))
        self.env["PREVIEW_TEST_API"] = "failed"
        result = self.run_script("release_preview.sh", "update")
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertTrue(any(command[:3] == ["systemctl", "--user", "restart"]
                            for command in self.commands()))


if __name__ == "__main__":
    unittest.main()
