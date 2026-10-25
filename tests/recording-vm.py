"""Validate guest workload selection without booting QEMU or accessing the network."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]


class RecordingVmTests(unittest.TestCase):
    def seed(self, **options):
        with tempfile.TemporaryDirectory() as directory:
            env = {key: value for key, value in os.environ.items()
                   if key != "WORKLOAD"}
            env.update(options)
            result = subprocess.run(
                ["python3", str(ROOT / "scripts/recording-vm/seed.py"),
                 directory, "a" * 40, "10191"],
                env=env, capture_output=True, text=True,
            )
            path = Path(directory) / "user-data"
            config = json.loads(path.read_text().split("\n", 1)[1]) if path.exists() else None
            return result, config

    def test_browser_workloads(self):
        for browser in (None, "chromium", "firefox"):
            with self.subTest(browser=browser):
                result, config = self.seed(**({"WORKLOAD": browser} if browser else {}))
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(config["runcmd"][0][2:], ["a" * 40, "10191", browser or "chromium"])
                self.assertNotIn("ccache", config["packages"])
                self.assertEqual(len(config["write_files"]), 1)

    def test_kernel_workload_is_self_contained(self):
        result, config = self.seed(WORKLOAD="kernel-build")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(config["runcmd"][0][-1], "kernel-build")
        self.assertNotIn("ccache", config["packages"])
        for dependency in ("flex", "bison", "libssl-dev", "bc"):
            self.assertIn(dependency, config["packages"])
        script = next(item for item in config["write_files"]
                      if item["path"] == "/opt/kernel-build.sh")
        self.assertEqual(script["permissions"], "0755")
        self.assertEqual(script["content"],
                         (ROOT / "scripts/recording-vm/kernel-build.sh").read_text())

    def test_invalid_workload_does_not_write_seed(self):
        result, config = self.seed(WORKLOAD="unknown")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("WORKLOAD must be", result.stderr)
        self.assertIsNone(config)


if __name__ == "__main__":
    unittest.main()
