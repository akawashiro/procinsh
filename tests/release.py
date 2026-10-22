#!/usr/bin/env python3
"""Offline release tests: real Git/worktrees, simulated GitHub/build/registry."""

import hashlib
from contextlib import redirect_stderr, redirect_stdout
import importlib.util
import io
import json
from pathlib import Path
import subprocess
import sys
import tarfile
import tempfile
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location("release", Path(__file__).resolve().parents[1] / "scripts/release.py")
release = importlib.util.module_from_spec(spec)
spec.loader.exec_module(release)


def crate(version, commit, payload=b"built frontend"):
    contents = {".cargo_vcs_info.json": json.dumps({"git": {"sha1": commit}}).encode(),
                "web/dist/index.html": payload}
    stream = io.BytesIO()
    with tarfile.open(fileobj=stream, mode="w:gz") as archive:
        for name, data in contents.items():
            member = tarfile.TarInfo(f"procinsh-{version}/{name}")
            member.size = len(data)
            archive.addfile(member, io.BytesIO(data))
    return stream.getvalue()


class ReleaseTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "checkout"
        self.root.mkdir()
        self.remote = Path(self.temp.name) / "remote.git"
        self.real_run = release.run
        patch.object(release, "REMOTE_URL", str(self.remote)).start()
        self.addCleanup(patch.stopall)
        self.git("init", "--initial-branch=main")
        self.git("config", "user.name", "Release Test")
        self.git("config", "user.email", "test@example.invalid")
        (self.root / "Cargo.toml").write_text('[package]\nname = "procinsh"\nversion = "0.1.13"\nedition = "2024"\n')
        (self.root / "Cargo.lock").write_text('version = 4\n\n[[package]]\nname = "other"\nversion = "0.1.13"\n\n[[package]]\nname = "procinsh"\nversion = "0.1.13"\n')
        (self.root / ".gitignore").write_text('/target/\n/web/dist/\n/web/node_modules/\n')
        self.git("add", ".")
        self.git("commit", "-m", "initial")
        self.git("init", "--bare", str(self.remote))
        self.git("remote", "add", "origin", str(self.remote))
        self.git("push", "origin", "main")
        self.initial = self.git("rev-parse", "HEAD")
        self.directory = self.root / ".git/procinsh-release"
        self.directory.mkdir()
        self.runner = release.Release(self.root, self.directory, "patch")
        self.directory = self.runner.directory
        self.calls = []
        self.pr = None
        self.registry = None
        self.uploads = 0
        self.fail_upload = False
        self.fail_release = False
        self.releases = []
        self.checks = [{"conclusion": "SUCCESS"}]
        self.views = 0
        self.merged = None
        patch.object(release, "run", self.command).start()
        patch.object(release, "published", lambda version: self.registry).start()
        patch.object(release, "download", lambda url: self.remote_archive).start()
        patch.object(release.time, "sleep", lambda seconds: None).start()

    def git(self, *args, cwd=None):
        return self.real_run("git", *args, cwd=cwd or self.root, capture=True)

    def command(self, *args, cwd, capture=False):
        self.calls.append(args)
        if args[0] == "git":
            return self.real_run(*args, cwd=cwd, capture=capture)
        if args[:3] == ("gh", "pr", "create"):
            self.pr = "https://github.com/akawashiro/procinsh/pull/999"
            return self.pr
        if args[:3] == ("gh", "release", "create"):
            tag = args[3]
            self.assertEqual(args[4:], ("--verify-tag", "--title", tag,
                                        "--notes", "[procinsh 0.1.14 on crates.io]"
                                        "(https://crates.io/crates/procinsh/0.1.14)\n\n",
                                        "--generate-notes", "--notes-start-tag", "v0.1.13",
                                        "--repo", release.REPO))
            self.assertEqual(self.uploads, 1)
            self.assertEqual(self.git("rev-parse", f"{tag}^{{commit}}"), self.merged)
            self.assertIn(self.merged, self.git(
                "ls-remote", str(self.remote), f"refs/tags/{tag}^{{}}"))
            if self.fail_release:
                raise subprocess.CalledProcessError(1, args)
            self.releases.append(tag)
            return f"https://github.com/{release.REPO}/releases/tag/{tag}"
        if args[:3] == ("gh", "repo", "view"):
            self.assertNotIn("--repo", args)
            return '{"viewerDefaultMergeMethod":"SQUASH"}'
        if args[:3] == ("gh", "pr", "view"):
            self.views += 1
            return json.dumps({"state": "MERGED" if self.merged else "OPEN",
                               "headRefOid": self.runner.info["head"], "baseRefName": "main",
                               "mergeCommit": {"oid": self.merged}, "mergeable": "MERGEABLE",
                               "mergeStateStatus": "CLEAN", "reviewDecision": "",
                               "statusCheckRollup": self.checks})
        if args[:3] == ("gh", "pr", "merge"):
            self.assertIn("--match-head-commit", args)
            self.assertIn("--squash", args)
            # Squash produces a commit distinct from the PR's head.
            self.git("checkout", "--detach", self.runner.info["base"], cwd=self.runner.work)
            self.git("merge", "--squash", self.runner.info["head"], cwd=self.runner.work)
            self.git("commit", "-m", "merged release", cwd=self.runner.work)
            self.merged = self.git("rev-parse", "HEAD", cwd=self.runner.work)
            self.git("push", "origin", f"{self.merged}:refs/heads/main")
            # Advance remote main again: publication must still use the PR merge.
            self.git("commit", "--allow-empty", "-m", "unrelated change", cwd=self.runner.work)
            self.git("push", "origin", "HEAD:refs/heads/main", cwd=self.runner.work)
            return ""
        if args[0] == "npm":
            if args[-1] == "build":
                (Path(cwd) / "web/dist").mkdir(parents=True, exist_ok=True)
                (Path(cwd) / "web/dist/index.html").write_text("built frontend")
            return ""
        if args[:2] == ("python3", "scripts/check_web_package.py"):
            archive = Path(cwd) / f"target/package/procinsh-{self.runner.info['version']}.crate"
            archive.parent.mkdir(parents=True, exist_ok=True)
            archive.write_bytes(crate(self.runner.info["version"], self.git("rev-parse", "HEAD", cwd=cwd)))
            return ""
        if args[:2] == ("cargo", "publish"):
            self.assertEqual(self.git("rev-parse", "HEAD", cwd=cwd), self.merged)
            self.uploads += 1
            self.remote_archive = (self.directory / "attempt.crate").read_bytes()
            self.registry = {"checksum": hashlib.sha256(self.remote_archive).hexdigest()}
            if self.fail_upload:
                raise subprocess.CalledProcessError(1, args)
            return ""
        self.fail(f"Unexpected command: {args}")

    def test_release_uses_fixed_remote_without_changing_origin(self):
        unused = str(self.root / "unused.git")
        self.git("remote", "set-url", "origin", unused)
        self.git("remote", "set-url", "--push", "origin", unused)
        self.runner.prepare()
        self.assertEqual(self.git("rev-parse", "origin/main"), self.initial)
        self.assertEqual(self.git("remote", "get-url", "origin"), unused)
        self.assertEqual(self.git("remote", "get-url", "--push", "origin"), unused)
        self.assertIn(self.runner.info["head"], self.git(
            "ls-remote", str(self.remote), f"refs/heads/{self.runner.info['branch']}"))

    def test_end_to_end_pins_merge_commit_and_preserves_checkout(self):
        (self.root / "local-work").write_text("keep me")
        output = io.StringIO()
        with redirect_stdout(output):
            self.runner.execute()
        self.assertEqual(self.git("rev-parse", "HEAD"), self.initial)
        self.assertEqual((self.root / "local-work").read_text(), "keep me")
        self.assertEqual(self.git("rev-parse", "v0.1.14^{commit}"), self.merged)
        self.assertNotEqual(self.merged, self.git("rev-parse", "origin/main"))
        self.assertEqual(self.uploads, 1)
        self.assertEqual(self.releases, ["v0.1.14"])
        self.assertIn("https://github.com/akawashiro/procinsh/releases/tag/v0.1.14", output.getvalue())
        self.assertFalse(self.runner.directory.exists())

    def test_release_failure_preserves_publication_tag_and_worktree(self):
        self.fail_release = True
        output = io.StringIO()
        with redirect_stdout(output), self.assertRaises(subprocess.CalledProcessError):
            self.runner.execute()
        self.assertIsNotNone(self.registry)
        self.assertEqual(self.git("rev-parse", "v0.1.14^{commit}"), self.merged)
        self.assertTrue(self.runner.work.exists())
        self.assertTrue((self.directory / "attempt.crate").exists())
        self.assertNotIn("Released v", output.getvalue())

    def test_failed_checks_stop_and_leave_worktree_for_inspection(self):
        self.checks = [{"conclusion": "FAILURE"}]
        with self.assertRaisesRegex(RuntimeError, "checks failed"):
            self.runner.execute()
        self.assertEqual(self.uploads, 0)
        self.assertTrue(self.runner.work.exists())
        previous_pr = self.runner.info["pr"]
        previous_work = self.runner.work
        self.runner = release.Release(self.root, self.directory.parent, "patch")
        self.assertNotEqual(self.runner.work, previous_work)
        self.checks = [{"conclusion": "SUCCESS"}]
        with self.assertRaisesRegex(RuntimeError, "branch .* already exists"):
            self.runner.execute()
        self.assertEqual(self.pr, previous_pr)
        self.assertEqual(self.uploads, 0)
        self.assertNotIn("pr", self.runner.info)

    def test_upload_failure_stops_without_tag_and_preserves_files(self):
        self.fail_upload = True
        with self.assertRaises(subprocess.CalledProcessError):
            self.runner.execute()
        self.assertEqual(self.uploads, 1)
        self.assertTrue(self.runner.work.exists())
        self.assertTrue((self.directory / "attempt.crate").exists())
        self.assertFalse(self.git("tag", "--list", "v0.1.14"))
        self.assertFalse(any(c[:3] == ("gh", "release", "create") for c in self.calls))
        self.assertFalse(list(self.directory.glob("*.json")))

    def test_changed_remote_archive_is_rejected(self):
        self.fail_upload = True
        with self.assertRaises(subprocess.CalledProcessError):
            self.runner.execute()
        self.remote_archive = crate("0.1.14", self.merged, b"unexpected")
        self.registry["checksum"] = hashlib.sha256(self.remote_archive).hexdigest()
        with self.assertRaisesRegex(RuntimeError, "differs"):
            release.verify_published("0.1.14", self.merged, self.directory / "attempt.crate", self.registry)

    def test_conflicting_tag_is_not_overwritten(self):
        self.runner.prepare()
        self.runner.merge()
        self.runner.publish()
        self.git("tag", "v0.1.14", self.initial)
        self.git("push", "origin", "refs/tags/v0.1.14")
        with self.assertRaisesRegex(RuntimeError, "another commit"):
            self.runner.tag()
        self.assertEqual(self.git("rev-parse", "v0.1.14"), self.initial)
        self.assertFalse(any(c[:3] == ("gh", "release", "create") for c in self.calls))

    def test_changed_pr_head_is_rejected(self):
        self.runner.prepare()
        original = self.runner.gh
        def gh(*args):
            if args[:2] == ("pr", "view"):
                return json.dumps({"headRefOid": "unexpected", "baseRefName": "main"})
            return original(*args)
        self.runner.gh = gh
        with self.assertRaisesRegex(RuntimeError, "head or base changed"):
            self.runner.merge()

    def test_version_updates_preserve_dependency_versions(self):
        release.set_versions(self.root, "0.1.13", "0.2.0")
        lock = release.tomllib.loads((self.root / "Cargo.lock").read_text())
        self.assertEqual([p["version"] for p in lock["package"]], ["0.1.13", "0.2.0"])

    def test_legacy_saved_state_is_ignored_and_worktree_path_logged(self):
        legacy = self.directory.parent / "current.json"
        legacy.write_text('{"kind":"minor","version":"9.9.9","complete":true}')
        logs = io.StringIO()
        with redirect_stderr(logs):
            fresh = release.Release(self.root, self.directory.parent, "patch")
        self.assertNotEqual(fresh.work, self.runner.work)
        self.assertIn(f"Release worktree: {fresh.work}", logs.getvalue())
        self.assertTrue(fresh.work.is_absolute())
        self.assertEqual(fresh.info, {})
        self.assertEqual(json.loads(legacy.read_text())["version"], "9.9.9")

    def test_checks_and_reviews_are_waited_for(self):
        self.runner.prepare()
        original = self.runner.gh
        def gh(*args):
            result = original(*args)
            if args[:2] == ("pr", "view") and self.views <= 3:
                pr = json.loads(result)
                if self.views == 1:
                    pr["statusCheckRollup"] = []  # checks not registered yet
                elif self.views == 2:
                    pr["statusCheckRollup"] = [{"status": "IN_PROGRESS", "conclusion": ""}]
                else:
                    pr["reviewDecision"] = "REVIEW_REQUIRED"
                    pr["mergeStateStatus"] = "BLOCKED"
                return json.dumps(pr)
            return result
        self.runner.gh = gh
        self.runner.merge()
        self.assertGreaterEqual(self.views, 5)
        self.assertEqual(sum(c[:3] == ("gh", "pr", "merge") for c in self.calls), 1)

    def test_unexpected_publication_stops_without_tag(self):
        self.runner.prepare()
        self.runner.merge()
        self.registry = {"checksum": "unexpected"}
        with self.assertRaisesRegex(RuntimeError, "already published"):
            self.runner.publish()
        self.assertEqual(self.uploads, 0)
        self.assertFalse(self.git("tag", "--list", "v0.1.14"))

    def test_interruption_preserves_worktree_without_persisting_state(self):
        self.runner.prepare()
        with patch.object(self.runner, "gh", side_effect=KeyboardInterrupt):
            with self.assertRaises(KeyboardInterrupt):
                self.runner.merge()
        self.assertTrue(self.runner.work.exists())
        self.assertFalse(list(self.directory.glob("*.json")))
        self.assertEqual(self.uploads, 0)


class FailureLogTest(unittest.TestCase):
    def test_failure_and_interruption_report_worktree_without_resume_instructions(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            common = root / ".git"
            common.mkdir()
            runner = unittest.mock.Mock(work=root / "run-test/worktree", directory=root / "run-test")

            def bootstrap(*args, **kwargs):
                if args[:3] == ("git", "rev-parse", "--show-toplevel"):
                    return str(root)
                if args[:2] == ("git", "rev-parse"):
                    return str(common)
                if args[:3] == ("gh", "auth", "status"):
                    return ""
                self.fail(f"Unexpected bootstrap command: {args}")

            for error, code in [(RuntimeError("test failure"), 1), (KeyboardInterrupt(), 130)]:
                with self.subTest(error=type(error).__name__):
                    logs = io.StringIO()
                    runner.execute.side_effect = error
                    with (redirect_stderr(logs),
                          patch.object(release, "run", side_effect=bootstrap),
                          patch.object(release, "Release", return_value=runner),
                          patch.object(release.shutil, "which", return_value="/test/tool"),
                          patch.object(sys, "argv", ["release.py", "patch"])):
                        with self.assertRaises(SystemExit) as raised:
                            release.main()
                    self.assertEqual(raised.exception.code, code)
                    self.assertIn(str(runner.work), logs.getvalue())
                    self.assertIn("manually", logs.getvalue())
                    self.assertNotIn("resume", logs.getvalue())
                    self.assertNotIn("rerun", logs.getvalue())


class CommandLogTest(unittest.TestCase):
    def test_capture_keeps_logs_out_of_result_and_stdout(self):
        logs = io.StringIO()
        stdout = io.StringIO()
        with redirect_stderr(logs), redirect_stdout(stdout):
            result = release.run(sys.executable, "-c", "print('captured output')",
                                 cwd=Path(tempfile.gettempdir()), capture=True)
        self.assertEqual(result, "captured output")
        self.assertEqual(stdout.getvalue(), "")
        lines = logs.getvalue().splitlines()
        self.assertEqual(len(lines), 2)
        self.assertRegex(lines[0], r"^\[\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ\] START cwd=")
        self.assertIn("command=" + release.shlex.join((sys.executable, "-c", "print('captured output')")), lines[0])
        self.assertRegex(lines[1], r"END exit=0 elapsed=\d+\.\d{2}s command=")

    def test_start_is_flushed_before_launch_and_end_after_completion(self):
        stream = unittest.mock.Mock()
        with patch.object(release.sys, "stderr", stream):
            def execute(*args, **kwargs):
                self.assertIn("START cwd=", stream.write.call_args_list[0].args[0])
                stream.flush.assert_called_once_with()
                return subprocess.CompletedProcess(args, 0)
            with patch.object(release.subprocess, "run", side_effect=execute):
                self.assertIsNone(release.run("npm", "--prefix", "web", "run", "build", cwd=Path("/tmp")))
        self.assertIn("END exit=0", "".join(call.args[0] for call in stream.write.call_args_list))
        self.assertEqual(stream.flush.call_count, 2)

    def test_nonzero_exit_is_logged_and_exception_preserved(self):
        logs = io.StringIO()
        with redirect_stderr(logs):
            with self.assertRaises(subprocess.CalledProcessError) as raised:
                release.run(sys.executable, "-c", "raise SystemExit(7)",
                            cwd=Path(tempfile.gettempdir()), capture=True)
        self.assertEqual(raised.exception.returncode, 7)
        self.assertIn("END exit=7 elapsed=", logs.getvalue())
        self.assertNotIn("END exit=0", logs.getvalue())

    def test_start_failure_is_logged_and_exception_preserved(self):
        logs = io.StringIO()
        with tempfile.TemporaryDirectory() as directory, redirect_stderr(logs):
            with self.assertRaises(FileNotFoundError):
                release.run(str(Path(directory) / "missing-executable"), cwd=Path(directory))
        self.assertIn("START_FAILED errno=2 elapsed=", logs.getvalue())
        self.assertNotIn("END exit=", logs.getvalue())

    def test_interruption_is_logged_and_propagated(self):
        logs = io.StringIO()
        with redirect_stderr(logs), patch.object(release.subprocess, "run", side_effect=KeyboardInterrupt):
            with self.assertRaises(KeyboardInterrupt):
                release.run("cargo", "publish", cwd=Path("/tmp"))
        self.assertIn("INTERRUPTED elapsed=", logs.getvalue())
        self.assertNotIn("END exit=", logs.getvalue())


class InterfaceTest(unittest.TestCase):
    def test_bumps(self):
        for kind, expected in [("patch", "0.1.14"), ("minor", "0.2.0"), ("major", "1.0.0")]:
            self.assertEqual(release.bump("0.1.13", kind), expected)

    def test_arguments_required_and_validated(self):
        script = Path(release.__file__).with_suffix(".sh")
        for args in [[], ["0.1.14"], ["patch", "extra"]]:
            result = subprocess.run(["bash", str(script), *args], capture_output=True)
            self.assertEqual(result.returncode, 2)


if __name__ == "__main__":
    unittest.main()
