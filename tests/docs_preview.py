#!/usr/bin/env python3
"""Exercise publication boundaries and multi-version reconciliation without GitHub writes."""
import importlib.util
import io
import json
from pathlib import Path
import stat
import subprocess
import warnings
import tempfile
import unittest
from unittest.mock import patch
import zipfile

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("publish_docs", ROOT / "scripts/publish_docs.py")
publish = importlib.util.module_from_spec(spec)
spec.loader.exec_module(publish)
REPO = "owner/project"
A, B, C = "a" * 40, "b" * 40, "c" * 40


def pr(number, sha, title="Example <title>"):
    return {"number": number, "title": title, "state": "open", "head": {"sha": sha, "repo": {"full_name": REPO}}}


def bundle(files):
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w") as archive:
        for name, value in files.items():
            archive.writestr(name, value)
    return output.getvalue()


class FakeGitHub:
    repository = REPO

    def __init__(self, prs=(), runs=(), main=A):
        self.prs = {p["number"]: p for p in prs}
        self.runs = list(runs)
        self.main = main
        self.artifacts = {}
        self.comments = {}
        self.writes = []

    def pages(self, path, key=None):
        if "/pulls?" in path:
            return iter(self.prs.values())
        if "/actions/artifacts?name=" in path:
            name = path.split("name=")[1]
            return iter(a for artifacts in self.artifacts.values() for a in artifacts if a["name"] == name)
        if path.endswith("/artifacts"):
            run_id = int(path.split("/")[-2])
            return iter(self.artifacts.get(run_id, []))
        if path.endswith("/comments"):
            return iter(self.comments.get(int(path.split("/")[-2]), []))
        raise AssertionError(path)

    def api(self, path, method="GET", payload=None):
        if method != "GET":
            self.writes.append((method, path, payload))
            if method == "POST":
                number = int(path.split("/")[-2])
                self.comments.setdefault(number, []).append({"id": number, "body": payload["body"], "user": {"login": "github-actions[bot]"}})
            else:
                identifier = int(path.split("/")[-1])
                for comments in self.comments.values():
                    for comment in comments:
                        if comment["id"] == identifier:
                            comment["body"] = payload["body"]
            return {}
        if path.endswith("/actions/workflows/docs-build.yml"):
            return {"id": 1}
        if "/actions/runs/" in path:
            return next(run for run in self.runs if run["id"] == int(path.split("/")[-1]))
        if path.endswith("/commits/main"):
            return {"sha": self.main}
        if "/pulls/" in path:
            return self.prs[int(path.split("/")[-1])]
        raise AssertionError(path)

    def download(self, identifier, target):
        target.write_bytes(bundle({"metadata.json": "{}", "site/index.html": f"version {identifier}",
                                   "site/typescript/index.html": "typescript", "site/rust-architecture/index.html": "graph"}))

    def add(self, key, sha, success=True, event=None, number=None):
        identifier = len(self.runs) + 1
        event = event or ("push" if key == "main" else "pull_request")
        self.runs.insert(0, {"id": identifier, "event": event, "head_branch": "main" if event != "pull_request" else "feature",
                             "head_sha": sha, "head_repository": {"full_name": REPO}, "pull_requests": [{"number": number}] if number else [],
                             "status": "completed", "workflow_id": 1, "conclusion": "success" if success else "failure", "html_url": f"https://github.com/{REPO}/actions/runs/{identifier}"})
        self.artifacts[identifier] = [{"id": identifier, "name": f"docs-{key}-{sha}", "expired": False, "workflow_run": {"id": identifier}}]


class PublicationTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.work = Path(self.temporary.name)
        self.root = self.work / "snapshot"
        self.site = self.root / "site"
        self.site.joinpath("main").mkdir(parents=True)
        self.site.joinpath("main/index.html").write_text("main unchanged")
        self.state = {"versions": {"main": {"sha": A, "run_url": "old"}}}
        self.loader = patch.object(publish, "load_snapshot", return_value=(self.root, self.state, A))
        self.loader.start()

    def tearDown(self):
        self.loader.stop()
        self.temporary.cleanup()

    def retained(self, number, sha):
        directory = self.site / "pr" / str(number)
        directory.mkdir(parents=True)
        directory.joinpath("index.html").write_text("retained preview")
        self.state["versions"][f"pr-{number}"] = {"sha": sha, "run_url": "old"}

    def test_main_update_and_multiple_prs_preserve_other_versions(self):
        self.retained(300, A)
        api = FakeGitHub([pr(288, B), pr(289, C), pr(300, A)], main=B)
        api.add("main", B)
        api.add("pr-288", B, number=288)
        api.add("pr-289", C, number=289)
        self.assertTrue(publish.prepare(api, self.work))
        self.assertIn("version", self.site.joinpath("main/index.html").read_text())
        self.assertIn("version", self.site.joinpath("pr/288/index.html").read_text())
        self.assertIn("version", self.site.joinpath("pr/289/index.html").read_text())
        self.assertEqual(self.site.joinpath("pr/300/index.html").read_text(), "retained preview")
        index = self.site.joinpath("index.html").read_text()
        self.assertIn("Example &lt;title&gt;", index)
        self.assertIn('href="main/"', index)
        self.assertIn('href="pr/288/"', index)

    def test_stale_and_failed_builds_keep_last_success(self):
        self.retained(288, A)
        api = FakeGitHub([pr(288, C)])
        api.add("pr-288", B, number=288)
        api.add("pr-288", C, number=288, success=False)
        self.assertTrue(publish.prepare(api, self.work))
        self.assertEqual(self.site.joinpath("pr/288/index.html").read_text(), "retained preview")
        self.assertEqual(self.site.joinpath("main/index.html").read_text(), "main unchanged")
        context = json.loads(self.work.joinpath("context.json").read_text())
        body = publish.comment_body(288, C, context["state"]["versions"]["pr-288"], context["results"]["pr-288"])
        self.assertIn("failed", body)
        self.assertIn(A, body)
        self.assertIn(C, body)

    def test_closed_pr_is_removed_and_delayed_build_cannot_restore_it(self):
        self.retained(288, B)
        api = FakeGitHub()
        api.add("pr-288", B, number=288)
        self.assertTrue(publish.prepare(api, self.work))
        self.assertFalse(self.site.joinpath("pr/288").exists())
        self.assertEqual(self.site.joinpath("main/index.html").read_text(), "main unchanged")
        self.assertNotIn("pr-288", self.state["versions"])

    def test_main_cannot_be_replaced_by_pr_artifact_and_forks_are_rejected(self):
        api = FakeGitHub([pr(288, B)], main=B)
        api.add("main", B, event="pull_request", number=288)
        api.add("pr-288", B, number=288)
        api.runs[0]["head_repository"]["full_name"] = "fork/project"
        publish.prepare(api, self.work)
        self.assertEqual(self.site.joinpath("main/index.html").read_text(), "main unchanged")
        self.assertFalse(self.site.joinpath("pr/288").exists())

    def test_dispatch_can_preview_a_branch_without_new_workflow(self):
        api = FakeGitHub([pr(288, B)])
        api.add("pr-288", B, event="workflow_dispatch")
        api.runs[0]["head_sha"] = A  # Dispatcher runs on main; selected code is PR head.
        self.assertTrue(publish.prepare(api, self.work))
        self.assertEqual(self.state["versions"]["pr-288"]["sha"], B)

    def test_comment_is_created_once_then_updated(self):
        api = FakeGitHub()
        body = publish.comment_body(288, B, {"sha": B}, None)
        publish.upsert_comment(api, 288, body)
        publish.upsert_comment(api, 288, body)
        publish.upsert_comment(api, 288, publish.comment_body(288, C, {"sha": B}, "failed"))
        self.assertEqual([write[0] for write in api.writes], ["POST", "PATCH"])
        publish.upsert_comment(api, 288, publish.comment_body(288, "", None, "closed"), create=False)
        self.assertEqual(api.writes[-1][0], "PATCH")
        self.assertIn("Removed", api.writes[-1][2]["body"])
        publish.upsert_comment(api, 289, "closed", create=False)
        self.assertEqual(len(api.writes), 3)

    def test_legacy_redirect_preserves_query_and_fragment(self):
        self.site.joinpath("main/demo/replay/space").mkdir(parents=True)
        self.site.joinpath("main/demo/replay/space/index.html").write_text("demo")
        publish.render_site(self.site, self.state, {})
        page = self.site.joinpath("demo/replay/space/index.html").read_text()
        self.assertIn("main/demo/replay/space/index.html", page)
        self.assertIn("+location.search+location.hash", page)
        self.assertEqual(self.site.joinpath("main/demo/replay/space/index.html").read_text(), "demo")

    def test_aggregate_size_and_links_rejected(self):
        self.assertRaises(ValueError, publish.validate_tree, self.site, 1)
        self.site.joinpath("bad").symlink_to("/etc/passwd")
        self.assertRaises(ValueError, publish.validate_tree, self.site)

    def test_snapshot_branch_keeps_one_commit_and_current_files(self):
        bare = self.work / "remote.git"
        subprocess.run(["git", "init", "--bare", "--quiet", str(bare)], check=True)
        original_git = publish.git
        def local_git(root, *args, **kwargs):
            args = tuple(str(bare) if arg == f"https://github.com/{REPO}.git" else arg for arg in args)
            return original_git(root, *args, **kwargs)
        api = FakeGitHub()
        subprocess.run(["git", "init", "--quiet"], cwd=self.root, check=True)
        context = {"state": self.state, "old": "", "prs": {}, "removed": [], "results": {}}
        self.work.joinpath("context.json").write_text(json.dumps(context))
        with patch.object(publish, "git", side_effect=local_git):
            publish.finalize(api, self.work, True)
        sha = subprocess.check_output(["git", "--git-dir", str(bare), "rev-parse", "docs-site"], text=True).strip()
        other = self.work / "second"
        clone = other / "snapshot"
        clone.mkdir(parents=True)
        subprocess.run(["git", "init", "--quiet"], cwd=clone, check=True)
        subprocess.run(["git", "fetch", "--quiet", str(bare), "docs-site"], cwd=clone, check=True)
        subprocess.run(["git", "checkout", "--quiet", "--detach", "FETCH_HEAD"], cwd=clone, check=True)
        clone.joinpath("site/main/index.html").write_text("updated main")
        context["old"] = sha
        other.joinpath("context.json").write_text(json.dumps(context))
        with patch.object(publish, "git", side_effect=local_git):
            publish.finalize(api, other, True)
        count = subprocess.check_output(["git", "--git-dir", str(bare), "rev-list", "--count", "docs-site"], text=True).strip()
        content = subprocess.check_output(["git", "--git-dir", str(bare), "show", "docs-site:site/main/index.html"], text=True)
        self.assertEqual(count, "1")
        self.assertEqual(content, "updated main")

    def test_archive_rejects_paths_links_duplicates_and_oversize(self):
        for index, name in enumerate(["/site/index.html", "../site/index.html", "site/../../x", "site\\x", "C:/site/x", ".git/config"]):
            with self.subTest(name=name):
                archive = io.BytesIO(bundle({name: "bad"}))
                self.assertRaises(ValueError, publish.safe_extract, archive, self.work / f"bad-{index}")
        output = io.BytesIO()
        with zipfile.ZipFile(output, "w") as archive:
            entry = zipfile.ZipInfo("site/link")
            entry.external_attr = (stat.S_IFLNK | 0o777) << 16
            archive.writestr(entry, "/etc/passwd")
        self.assertRaises(ValueError, publish.safe_extract, io.BytesIO(output.getvalue()), self.work / "symlink")
        self.assertRaises(ValueError, publish.safe_extract, io.BytesIO(bundle({"site/index.html": "long"})), self.work / "large", 1)
        duplicate = io.BytesIO()
        with warnings.catch_warnings():
            warnings.simplefilter("ignore", UserWarning)
            with zipfile.ZipFile(duplicate, "w") as archive:
                archive.writestr("site/index.html", "first")
                archive.writestr("site/index.html", "second")
        self.assertRaises(ValueError, publish.safe_extract, io.BytesIO(duplicate.getvalue()), self.work / "duplicate")
        destination = self.work / "valid"
        publish.safe_extract(io.BytesIO(bundle({"metadata.json": "{}", "site/index.html": "ok"})), destination)
        self.assertEqual(destination.joinpath("site/index.html").read_text(), "ok")


if __name__ == "__main__":
    unittest.main()
