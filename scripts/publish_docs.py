#!/usr/bin/env python3
"""Reconcile static documentation snapshots using trusted main-branch code only."""
import argparse
import html
import json
import os
from pathlib import Path, PurePosixPath
import re
import shutil
import stat
import subprocess
import zipfile

SITE = "https://akawashiro.github.io/procinsh/"
MAX_BYTES = 1_000_000_000
BRANCH = "docs-site"
MARKER = "<!-- procinsh-docs-preview -->"
ARTIFACT = re.compile(r"docs-(main|pr-([1-9][0-9]*))-([0-9a-f]{40})\Z")


class GitHub:
    def __init__(self, repository):
        if not re.fullmatch(r"[\w.-]+/[\w.-]+", repository):
            raise ValueError("Invalid repository")
        self.repository = repository

    def api(self, path, method="GET", payload=None):
        args = ["gh", "api", path, "--method", method]
        if payload is not None:
            args += ["--input", "-"]
        result = subprocess.run(args, input=json.dumps(payload) if payload is not None else None,
                                text=True, capture_output=True)
        if result.returncode:
            raise RuntimeError(result.stderr.strip())
        return json.loads(result.stdout) if result.stdout.strip() else None

    def pages(self, path, key=None):
        page = 1
        while True:
            value = self.api(path + ("&" if "?" in path else "?") + f"per_page=100&page={page}")
            entries = value[key] if key else value
            yield from entries
            if len(entries) < 100:
                break
            page += 1

    def download(self, artifact_id, target):
        with target.open("wb") as output:
            subprocess.run(["gh", "api", f"repos/{self.repository}/actions/artifacts/{int(artifact_id)}/zip"],
                           stdout=output, check=True)


def safe_extract(archive, destination, limit=MAX_BYTES):
    """Extract only bounded regular files under site/ and metadata.json."""
    destination.mkdir(parents=True, exist_ok=True)
    total, seen = 0, set()
    with zipfile.ZipFile(archive) as bundle:
        for entry in bundle.infolist():
            name = entry.filename
            path = PurePosixPath(name)
            mode = entry.external_attr >> 16
            if (not name or name.startswith("/") or "\\" in name or ":" in path.parts[0]
                    or any(part in (".", "..", "", ".git", ".gitattributes", ".gitmodules", ".lfsconfig") for part in name.rstrip("/").split("/"))
                    or name in seen or stat.S_IFMT(mode) not in (0, stat.S_IFREG, stat.S_IFDIR)
                    or not (name == "metadata.json" or path.parts[0] == "site")):
                raise ValueError("Unsafe documentation artifact path or file type")
            seen.add(name)
            total += entry.file_size
            if total > limit:
                raise ValueError("Documentation artifact exceeds site size limit")
            target = destination.joinpath(*path.parts)
            if entry.is_dir():
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                with bundle.open(entry) as source, target.open("xb") as output:
                    shutil.copyfileobj(source, output)


def validate_tree(root, limit=MAX_BYTES):
    total = 0
    for path in root.rglob("*"):
        if path.is_symlink() or not (path.is_file() or path.is_dir()):
            raise ValueError("Published documentation must contain only regular files")
        if path.is_file():
            total += path.stat().st_size
            if total > limit:
                raise ValueError("Published documentation exceeds site size limit")
    return total


def git(root, *args, capture=False):
    result = subprocess.run(["git", "-c", "credential.helper=", "-c",
                             "credential.helper=!gh auth git-credential", *args],
                            cwd=root, check=True, text=True,
                            stdout=subprocess.PIPE if capture else None)
    return result.stdout.strip() if capture else None


def load_snapshot(api, work):
    root = work / "snapshot"
    root.mkdir(parents=True)
    git(root, "init", "--quiet")
    remote = f"https://github.com/{api.repository}.git"
    refs = api.api(f"repos/{api.repository}/git/matching-refs/heads/{BRANCH}")
    ref = next((ref for ref in refs if ref["ref"] == f"refs/heads/{BRANCH}"), None)
    old = ref["object"]["sha"] if ref else ""
    if old:
        git(root, "fetch", "--depth=1", remote, f"refs/heads/{BRANCH}")
        git(root, "checkout", "--detach", "FETCH_HEAD")
    state = json.loads((root / "state.json").read_text()) if (root / "state.json").exists() else {"versions": {}}
    (root / "site").mkdir(exist_ok=True)
    validate_tree(root / "site")
    return root, state, old


def eligible(run, key, sha, open_prs, main_sha, repository):
    if run.get("head_repository", {}).get("full_name") != repository:
        return False
    event = run["event"]
    if key == "main":
        return (event in ("push", "workflow_dispatch") and run["head_branch"] == "main"
                and sha == main_sha == run["head_sha"])
    number = int(key.removeprefix("pr-"))
    pr = open_prs.get(number)
    if not pr or pr["head"]["sha"] != sha:
        return False
    if event == "pull_request":
        return (run["head_sha"] == sha and any(p["number"] == number for p in run.get("pull_requests", [])))
    # Only trusted main dispatches may explicitly select older PR branches.
    return event == "workflow_dispatch" and run["head_branch"] == "main"


def replace_version(root, state, key, sha, source, run_url):
    if not (source / "index.html").is_file():
        raise ValueError("Documentation artifact has no index page")
    target = root / ("main" if key == "main" else f"pr/{int(key[3:])}")
    shutil.rmtree(target, ignore_errors=True)
    shutil.copytree(source, target)
    state["versions"][key] = {"sha": sha, "run_url": run_url}


def remove_closed(root, state, open_prs):
    removed = []
    for key in list(state["versions"]):
        if key.startswith("pr-") and int(key[3:]) not in open_prs:
            shutil.rmtree(root / "pr" / key[3:], ignore_errors=True)
            del state["versions"][key]
            removed.append(int(key[3:]))
    return removed


def render_site(root, state, open_prs):
    # Legacy entry points redirect to main while retaining recording parameters
    # and Rust item fragments. Non-version directories contain only these stubs.
    for path in list(root.iterdir()):
        if path.name not in ("main", "pr"):
            if path.is_dir():
                shutil.rmtree(path)
            else:
                path.unlink()
    main = root / "main"
    if main.exists():
        for page in main.rglob("*.html"):
            relative = page.relative_to(main)
            if relative == Path("index.html") or relative.parts[0] in ("main", "pr"):
                continue
            target = root / relative
            target.parent.mkdir(parents=True, exist_ok=True)
            url = SITE + "main/" + relative.as_posix()
            target.write_text('<!doctype html><meta charset="utf-8">'
                              f'<meta http-equiv="refresh" content="0; url={html.escape(url)}">'
                              f'<a href="{html.escape(url)}">Documentation moved</a>'
                              '<script>location.replace(' + json.dumps(url)
                              + '+location.search+location.hash)</script>')
    links = ['<li><a href="main/">main documentation and replay demo</a></li>']
    for key, value in sorted(state["versions"].items()):
        if key == "main":
            continue
        number = int(key[3:])
        title = html.escape(open_prs[number]["title"])
        links.append(f'<li><a href="pr/{number}/">PR #{number}: {title}</a> — {value["sha"][:12]}</li>')
    root.joinpath("index.html").write_text(
        '<!doctype html><html lang="en"><meta charset="utf-8">'
        '<meta name="viewport" content="width=device-width,initial-scale=1">'
        '<title>procinsh documentation versions</title><h1>Documentation versions</h1><ul>'
        + "".join(links) + '</ul></html>')
    root.joinpath(".nojekyll").touch()


def comment_body(number, current_sha, published, result):
    lines = [MARKER, "### Documentation preview", ""]
    if result == "closed":
        return "\n".join(lines + ["Removed because this pull request is closed."])
    if published:
        lines += [f"[Open PR documentation]({SITE}pr/{number}/)", "",
                  f"Published commit: `{published['sha']}`."]
    else:
        lines += ["No preview has been published yet."]
    if result:
        lines += ["", f"Latest build/publication: {result}.", f"Requested commit: `{current_sha}`."]
    elif published and published["sha"] != current_sha:
        lines += ["", f"Waiting for commit `{current_sha}`; the link still shows the published commit above."]
    return "\n".join(lines)


def upsert_comment(api, number, body, create=True):
    comments = api.pages(f"repos/{api.repository}/issues/{number}/comments")
    existing = next((comment for comment in comments if MARKER in comment["body"]
                     and comment["user"]["login"] == "github-actions[bot]"), None)
    if existing:
        if existing["body"] != body:
            api.api(f"repos/{api.repository}/issues/comments/{existing['id']}", "PATCH", {"body": body})
    elif create:
        api.api(f"repos/{api.repository}/issues/{number}/comments", "POST", {"body": body})


def prepare(api, work):
    root, state, old = load_snapshot(api, work)
    site = root / "site"
    prs = {p["number"]: p for p in api.pages(f"repos/{api.repository}/pulls?state=open")
           if p["head"].get("repo") and p["head"]["repo"]["full_name"] == api.repository}
    main_sha = api.api(f"repos/{api.repository}/commits/main")["sha"]
    previous = json.loads(json.dumps(state["versions"]))
    removed = remove_closed(site, state, prs)
    wanted = {"main": main_sha, **{f"pr-{n}": p["head"]["sha"] for n, p in prs.items()}}
    # Already published commits survive artifact expiration. Reconcile every PR
    # on each run: GitHub concurrency may coalesce pending publisher executions.
    seen = {key for key, sha in wanted.items() if state["versions"].get(key, {}).get("sha") == sha}
    results = {}
    context = {"state": state, "old": old, "prs": {str(n): p["head"]["sha"] for n, p in prs.items()},
               "removed": removed, "results": results, "previous_versions": previous}
    def save_context():
        (work / "context.json").write_text(json.dumps(context))
    save_context()
    workflow_id = api.api(f"repos/{api.repository}/actions/workflows/docs-build.yml")["id"]
    for key, sha in wanted.items():
        if key in seen:
            continue
        name = f"docs-{key}-{sha}"
        artifacts = sorted(api.pages(f"repos/{api.repository}/actions/artifacts?name={name}", "artifacts"),
                           key=lambda artifact: artifact["id"], reverse=True)
        for artifact in artifacts:
            if artifact["name"] != name or artifact["expired"]:
                continue
            run = api.api(f"repos/{api.repository}/actions/runs/{artifact['workflow_run']['id']}")
            if (run["status"] != "completed" or run["workflow_id"] != workflow_id
                    or not eligible(run, key, sha, prs, main_sha, api.repository)):
                continue
            if run["conclusion"] != "success":
                results[key] = "build failed; previous preview retained"
                break
            results[key] = "publication pending"
            save_context()
            archive = work / f"artifact-{artifact['id']}.zip"
            api.download(artifact["id"], archive)
            extracted = work / f"artifact-{artifact['id']}"
            safe_extract(archive, extracted)
            replace_version(site, state, key, sha, extracted / "site", run["html_url"])
            results[key] = "published"
            break
    # Recheck after downloads so close events and newer commits cannot resurrect
    # closed previews or publish a build that became stale during preparation.
    for key, result in list(results.items()):
        if not key.startswith("pr-") or not result.startswith("published"):
            continue
        number = int(key[3:])
        current = api.api(f"repos/{api.repository}/pulls/{number}")
        if current["state"] != "open" or current["head"]["sha"] != state["versions"][key]["sha"]:
            raise ValueError("PR changed during publication; retry reconciliation")
    render_site(site, state, prs)
    validate_tree(site)
    save_context()
    return "main" in state["versions"]


def finalize(api, work, ready):
    context = json.loads((work / "context.json").read_text())
    root, state = work / "snapshot", context["state"]
    if ready:
        (root / "state.json").write_text(json.dumps(state, sort_keys=True, indent=2) + "\n")
        git(root, "checkout", "--orphan", "snapshot")
        git(root, "config", "user.name", "github-actions[bot]")
        git(root, "config", "user.email", "41898282+github-actions[bot]@users.noreply.github.com")
        git(root, "add", "--all")
        git(root, "commit", "-m", "Update documentation snapshots")
        git(root, "push", f"--force-with-lease=refs/heads/{BRANCH}:{context['old']}",
            f"https://github.com/{api.repository}.git", f"HEAD:refs/heads/{BRANCH}")
    for number, sha in context["prs"].items():
        key = f"pr-{number}"
        published = state["versions"].get(key) if ready else None
        result = context["results"].get(key)
        if not ready and result and result.startswith("published"):
            result = "built; waiting for the first successful main publication"
        if published or result:
            upsert_comment(api, int(number), comment_body(int(number), sha, published, result))
    for number in context["removed"]:
        upsert_comment(api, number, comment_body(number, "", None, "closed"), create=False)


def report_error(api, work):
    path = work / "context.json"
    if not path.exists():
        return
    context = json.loads(path.read_text())
    for key in context["results"]:
        if key.startswith("pr-"):
            number = int(key[3:])
            upsert_comment(api, number, comment_body(number, context["prs"][str(number)], context["previous_versions"].get(key),
                           "publication failed; previous preview retained"))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["prepare", "finalize", "error"])
    parser.add_argument("--workspace", type=Path, required=True)
    parser.add_argument("--ready", action="store_true")
    args = parser.parse_args()
    args.workspace.mkdir(parents=True, exist_ok=True)
    api = GitHub(os.environ["GITHUB_REPOSITORY"])
    if args.command == "prepare":
        ready = prepare(api, args.workspace)
        with open(os.environ["GITHUB_OUTPUT"], "a") as output:
            output.write(f"ready={'true' if ready else 'false'}\n")
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a") as output:
            output.write(f"Documentation versions: {SITE}\n")
    elif args.command == "finalize":
        finalize(api, args.workspace, args.ready)
    else:
        report_error(api, args.workspace)


if __name__ == "__main__":
    main()
