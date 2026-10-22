#!/usr/bin/env python3
"""Release from origin/main in a separate worktree."""

import argparse
from datetime import datetime, timezone
import fcntl
import hashlib
import io
import json
import os
from pathlib import Path
import re
import shlex
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time
import tomllib
import urllib.error
import urllib.request


REPO = "akawashiro/procinsh"
REMOTE_URL = f"https://github.com/{REPO}.git"
PACKAGE = "procinsh"


def log(message):
    timestamp = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    print(f"[{timestamp}] {message}", file=sys.stderr, flush=True)


def run(*args, cwd, capture=False):
    env = dict(os.environ)
    env.pop("PROCINSH_GIT_SHA", None)
    env["CARGO_TARGET_DIR"] = str(Path(cwd) / "target")
    command = shlex.join(args)

    started = time.monotonic()
    log(f"START cwd={shlex.quote(str(cwd))} command={command}")
    try:
        result = subprocess.run(args, cwd=cwd, env=env, check=True, text=True,
                                stdout=subprocess.PIPE if capture else None)
    except subprocess.CalledProcessError as error:
        log(f"END exit={error.returncode} elapsed={time.monotonic() - started:.2f}s command={command}")
        raise
    except OSError as error:
        log(f"START_FAILED errno={error.errno} elapsed={time.monotonic() - started:.2f}s command={command}")
        raise
    except KeyboardInterrupt:
        log(f"INTERRUPTED elapsed={time.monotonic() - started:.2f}s command={command}")
        raise
    log(f"END exit={result.returncode} elapsed={time.monotonic() - started:.2f}s command={command}")
    return result.stdout.strip() if capture else None


def bump(version, kind):
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        raise RuntimeError(f"Expected a stable version, got {version!r}")
    parts = list(map(int, version.split(".")))
    index = {"major": 0, "minor": 1, "patch": 2}[kind]
    parts[index] += 1
    parts[index + 1:] = [0] * (2 - index)
    return ".".join(map(str, parts))


def set_versions(root, previous, version):
    """Change only the package's own version, preserving all other formatting."""
    for filename, section in [("Cargo.toml", "package"), ("Cargo.lock", "[package]")]:
        path = root / filename
        original = path.read_text()
        parsed = tomllib.loads(original)
        package = (parsed["package"] if filename == "Cargo.toml" else
                   next(p for p in parsed["package"] if p["name"] == PACKAGE))
        if package["version"] not in (previous, version):
            raise RuntimeError(f"Unexpected {filename} version: {package['version']}")
        chunks = re.split(r"(?m)(?=^\[)", original)
        found = 0
        for index, chunk in enumerate(chunks):
            if not chunk.startswith(f"[{section}]\n"):
                continue
            data = tomllib.loads(chunk)["package"]
            if isinstance(data, list):
                data = data[0]
            if data.get("name") == PACKAGE:
                chunks[index], count = re.subn(
                    r'(?m)^version = "[^"]+"$', f'version = "{version}"', chunk)
                found += count
        if found != 1:
            raise RuntimeError(f"Could not locate exactly one package version in {filename}")
        path.write_text("".join(chunks))


def download(url):
    request = urllib.request.Request(url, headers={"User-Agent": "procinsh-release (github.com/akawashiro/procinsh)"})
    with urllib.request.urlopen(request, timeout=60) as response:
        return response.read()


def published(version):
    try:
        return json.loads(download(f"https://crates.io/api/v1/crates/{PACKAGE}/{version}"))["version"]
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return None
        raise


def archive_files(data):
    with tarfile.open(fileobj=io.BytesIO(data), mode="r:gz") as archive:
        return {member.name: archive.extractfile(member).read()
                for member in archive.getmembers() if member.isfile()}


def verify_published(version, commit, expected, metadata):
    data = download(f"https://static.crates.io/crates/{PACKAGE}/{PACKAGE}-{version}.crate")
    if hashlib.sha256(data).hexdigest() != metadata["checksum"]:
        raise RuntimeError("Downloaded crate does not match the registry checksum")
    files = archive_files(data)
    vcs = json.loads(files[f"{PACKAGE}-{version}/.cargo_vcs_info.json"])
    if vcs["git"]["sha1"] != commit or files != archive_files(expected.read_bytes()):
        raise RuntimeError("Published crate differs from this release attempt; inspect it before continuing")


class Release:
    def __init__(self, root, directory, kind):
        self.root = root
        self.directory = Path(tempfile.mkdtemp(prefix="run-", dir=directory)).resolve()
        self.work = self.directory / "worktree"
        self.kind = kind
        self.info = {}
        log(f"Release worktree: {self.work}")

    def git(self, *args, work=False):
        # Use gh's credentials without changing persistent Git config.
        return run("git",
                   "-c", "credential.https://github.com.helper=",
                   "-c", "credential.https://github.com.helper=!gh auth git-credential",
                   *args, cwd=self.work if work else self.root, capture=True)

    def gh(self, *args):
        return run("gh", *args, "--repo", REPO, cwd=self.root, capture=True)

    def ensure_clean(self):
        if self.git("status", "--porcelain", "--untracked-files=all", work=True):
            raise RuntimeError(f"Unexpected files in release worktree {self.work}; inspect manually")

    def build(self, expected):
        self.ensure_clean()
        if self.git("rev-parse", "HEAD", work=True) != expected:
            raise RuntimeError("Release worktree is at an unexpected commit")
        # Remove ignored output too, so a previous build cannot contribute assets.
        shutil.rmtree(self.work / "web/dist", ignore_errors=True)
        for command in [("npm", "--prefix", "web", "ci"),
                        ("npm", "--prefix", "web", "run", "build"),
                        ("python3", "scripts/check_web_package.py")]:
            run(*command, cwd=self.work)
        self.ensure_clean()
        if self.git("rev-parse", "HEAD", work=True) != expected:
            raise RuntimeError("Build changed the release worktree commit")

    def prepare(self):
        self.git("fetch", REMOTE_URL, "+refs/heads/main:refs/remotes/origin/main", "--tags")
        base = self.git("rev-parse", "origin/main")
        manifest = tomllib.loads(self.git("show", f"{base}:Cargo.toml"))
        previous = manifest["package"]["version"]
        version = bump(previous, self.kind)
        branch = f"release/v{version}"
        if published(version):
            raise RuntimeError(f"{version} is already published; reconcile origin/main first")
        if self.git("tag", "--list", f"v{version}"):
            raise RuntimeError(f"Tag v{version} already exists")
        if self.git("ls-remote", "--heads", REMOTE_URL, f"refs/heads/{branch}"):
            raise RuntimeError(f"Release branch {branch} already exists; inspect the previous attempt manually")
        self.info.update(previous=previous, version=version, base=base, branch=branch)
        log(f"Releasing {version} ({self.kind}); worktree: {self.work}")
        self.git("worktree", "add", "--detach", str(self.work), base)
        set_versions(self.work, previous, version)
        self.git("add", "Cargo.toml", "Cargo.lock", work=True)
        self.git("commit", "-m", f"Release v{version}", work=True)
        head = self.git("rev-parse", "HEAD", work=True)
        self.validate_version_commit(head)
        self.info.update(head=head)
        self.build(head)
        self.git("push", REMOTE_URL, f"{head}:refs/heads/{branch}")
        body = self.directory / "pr-body.md"
        body.write_text(f"Release procinsh {version}.\n\n"
                        "Updates the package version and lockfile. Local Web build and "
                        "crate packaging verification passed before PR creation.\n")
        url = self.gh("pr", "create", "--base", "main", "--head", branch,
                      "--title", f"Release v{version}", "--body-file", str(body))
        self.info.update(pr=url)

    def validate_version_commit(self, head):
        if self.git("rev-parse", f"{head}^", work=True) != self.info["base"]:
            raise RuntimeError("Unexpected release commit ancestry")
        changed = set(self.git("diff", "--name-only", self.info["base"], head, work=True).splitlines())
        if changed != {"Cargo.toml", "Cargo.lock"}:
            raise RuntimeError("Release commit must change only Cargo.toml and Cargo.lock")
        # Compare against the exact expected patch, not just its file names.
        for filename in changed:
            expected = self.git("show", f"{self.info['base']}:{filename}", work=True)
            actual = self.git("show", f"{head}:{filename}", work=True)
            pattern = rf'(?m)^(name = "{PACKAGE}"\nversion = "){re.escape(self.info["previous"])}("$)'
            expected, count = re.subn(pattern, rf'\g<1>{self.info["version"]}\2', expected)
            if count != 1 or expected != actual:
                raise RuntimeError(f"Unexpected release changes in {filename}")

    def merge(self):
        fields = "state,headRefOid,baseRefName,mergeCommit,mergeable,mergeStateStatus,reviewDecision,statusCheckRollup"
        requested = False
        while True:
            pr = json.loads(self.gh("pr", "view", self.info["pr"], "--json", fields))
            if pr["headRefOid"] != self.info["head"] or pr["baseRefName"] != "main":
                raise RuntimeError("Release PR head or base changed; inspect manually")
            if pr["state"] == "MERGED":
                self.info.update(commit=pr["mergeCommit"]["oid"])
                break
            if pr["state"] == "CLOSED":
                raise RuntimeError("Release PR was closed without merging")
            if pr["mergeable"] == "CONFLICTING":
                raise RuntimeError("Release PR conflicts with main; resolve manually")
            checks = pr["statusCheckRollup"] or []
            failed = {"FAILURE", "CANCELLED", "TIMED_OUT", "ACTION_REQUIRED", "STARTUP_FAILURE", "ERROR"}
            if any((c.get("conclusion") or c.get("state")) in failed for c in checks):
                raise RuntimeError(f"PR checks failed: {self.info['pr']}; inspect the PR manually")
            passed = checks and all(c.get("conclusion") in {"SUCCESS", "NEUTRAL", "SKIPPED"}
                                    or c.get("state") == "SUCCESS" for c in checks)
            if passed and not requested:
                settings = json.loads(run("gh", "repo", "view", REPO, "--json", "viewerDefaultMergeMethod",
                                          cwd=self.root, capture=True))
                method = str(settings["viewerDefaultMergeMethod"]).lower()
                if method not in {"merge", "squash", "rebase"}:
                    raise RuntimeError(f"Unsupported default merge method: {method}")
                if pr["mergeStateStatus"] == "BEHIND":
                    raise RuntimeError("Release PR must be updated with main; inspect manually")
                # Poll approvals locally so repository auto-merge need not be enabled.
                # gh pr merge also enqueues a ready PR when a merge queue is required.
                if (pr["reviewDecision"] not in {"REVIEW_REQUIRED", "CHANGES_REQUESTED"}
                        and pr["mergeStateStatus"] not in {"BLOCKED", "UNKNOWN", "DRAFT"}):
                    self.gh("pr", "merge", self.info["pr"], f"--{method}",
                            "--match-head-commit", self.info["head"])
                    requested = True
            print(f"Waiting for PR checks/approval/merge: {self.info['pr']} (Ctrl-C to stop)", flush=True)
            time.sleep(10)

    def publish(self):
        version = self.info["version"]
        commit = self.info["commit"]
        attempt = self.directory / "attempt.crate"
        if published(version):
            raise RuntimeError(f"{version} is already published; inspect manually")
        self.git("fetch", REMOTE_URL, "+refs/heads/main:refs/remotes/origin/main")
        self.git("merge-base", "--is-ancestor", commit, "origin/main")
        self.ensure_clean()
        self.git("checkout", "--detach", commit, work=True)
        self.build(commit)
        self.ensure_clean()
        manifest = tomllib.loads((self.work / "Cargo.toml").read_text())
        if manifest["package"]["version"] != version:
            raise RuntimeError("Merged commit has the wrong version")
        archive = self.work / f"target/package/{PACKAGE}-{version}.crate"
        vcs = json.loads(archive_files(archive.read_bytes())[f"{PACKAGE}-{version}/.cargo_vcs_info.json"])
        if vcs["git"]["sha1"] != commit:
            raise RuntimeError("Packaged crate has the wrong Git commit; refusing to publish")
        shutil.copyfile(archive, attempt)
        print(f"Publishing {version} from {commit}", flush=True)
        run("cargo", "publish", "--locked", "--allow-dirty", "--registry", "crates-io", cwd=self.work)
        metadata = published(version)
        if not metadata:
            raise RuntimeError("Upload finished but registry confirmation is pending; check crates.io manually")
        verify_published(version, commit, attempt, metadata)

    def tag(self):
        tag = f"v{self.info['version']}"
        commit = self.info["commit"]
        local = self.git("tag", "--list", tag)
        if local and self.git("rev-parse", f"{tag}^{{commit}}") != commit:
            raise RuntimeError(f"Local tag {tag} points to another commit; it will not be overwritten")
        remote = self.git("ls-remote", "--tags", REMOTE_URL, f"refs/tags/{tag}", f"refs/tags/{tag}^{{}}")
        if remote:
            refs = dict(line.split()[::-1] for line in remote.splitlines())
            if refs.get(f"refs/tags/{tag}^{{}}", refs.get(f"refs/tags/{tag}")) != commit:
                raise RuntimeError(f"Remote tag {tag} points to another commit; it will not be overwritten")
        else:
            if not local:
                self.git("tag", "-a", tag, commit, "-m", f"Release {tag}")
            self.git("push", REMOTE_URL, f"refs/tags/{tag}")
        url = self.gh("release", "create", tag, "--verify-tag", "--title", tag,
                      "--notes", f"[{PACKAGE} {self.info['version']} on crates.io]"
                      f"(https://crates.io/crates/{PACKAGE}/{self.info['version']})\n\n",
                      "--generate-notes", "--notes-start-tag", f"v{self.info['previous']}")
        self.ensure_clean()
        self.git("worktree", "remove", str(self.work))
        shutil.rmtree(self.directory)
        print(f"Released {tag}: https://crates.io/crates/{PACKAGE}/{self.info['version']}", flush=True)
        print(f"GitHub Release: {url}", flush=True)

    def execute(self):
        self.prepare()
        self.merge()
        self.publish()
        self.tag()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("kind", choices=["patch", "minor", "major"])
    args = parser.parse_args()
    root = Path(run("git", "rev-parse", "--show-toplevel", cwd=Path(__file__).resolve().parent, capture=True))
    common = Path(run("git", "rev-parse", "--path-format=absolute", "--git-common-dir", cwd=root, capture=True))
    directory = common / "procinsh-release"
    directory.mkdir(exist_ok=True)
    with (directory / "lock").open("w") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            parser.exit(1, "Another release is running for this repository.\n")
        current = None
        try:
            for tool in ["git", "gh", "npm", "cargo"]:
                if not shutil.which(tool):
                    raise RuntimeError(f"Required executable not found: {tool}")
            run("gh", "auth", "status", cwd=root)
            current = Release(root, directory, args.kind)
            current.execute()
        except (RuntimeError, subprocess.CalledProcessError, OSError, ValueError, KeyError, tarfile.TarError) as error:
            if current is not None:
                log(f"Release files retained for inspection: {current.directory}; worktree: {current.work}")
            parser.exit(1, f"Release stopped: {error}\nInspect any created PR, publication, or tag manually.\n")
        except KeyboardInterrupt:
            if current is not None:
                log(f"Release files retained for inspection: {current.directory}; worktree: {current.work}")
            parser.exit(130, "\nRelease interrupted. Inspect any created PR, publication, or tag manually.\n")


if __name__ == "__main__":
    main()
