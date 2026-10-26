#!/usr/bin/env python3
"""Build a commit-pinned documentation artifact without publication privileges."""
import argparse
import html
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

SITE = "https://akawashiro.github.io/procinsh/"
SHA = re.compile(r"[0-9a-f]{40}\Z")


def gh(path):
    return json.loads(subprocess.check_output(["gh", "api", path], text=True))


def resolve(event, requested, repository):
    number = None
    if "pull_request" in event:
        number = event["pull_request"]["number"]
    elif requested:
        if not re.fullmatch(r"[1-9][0-9]*", requested):
            raise ValueError("PR number must be a positive integer")
        number = int(requested)
    if number:
        pr = gh(f"repos/{repository}/pulls/{number}")
        if pr["state"] != "open" or pr["head"]["repo"]["full_name"] != repository:
            raise ValueError("Only open, same-repository pull requests can be previewed")
        sha = pr["head"]["sha"]
        key = f"pr-{number}"
    else:
        sha = gh(f"repos/{repository}/commits/main")["sha"]
        key = "main"
    if not SHA.fullmatch(sha):
        raise ValueError("Invalid source commit")
    return {"key": key, "sha": sha, "number": number, "repository": repository}


def run(args, cwd, **kwargs):
    subprocess.run(args, cwd=cwd, check=True, **kwargs)


def build(source, artifact, metadata):
    source, artifact = source.resolve(), artifact.resolve()
    key, sha, repo = metadata["key"], metadata["sha"], metadata["repository"]
    base = SITE + ("main/" if key == "main" else f"pr/{metadata['number']}/")
    out = source / "target/doc"
    shutil.rmtree(out, ignore_errors=True)
    run(["npm", "--prefix", "web", "ci"], source)
    run(["npm", "--prefix", "web", "run", "build"], source)
    run(["cargo", "doc", "--locked", "--workspace", "--no-deps", "--document-private-items"], source,
        env={**os.environ, "RUSTDOCFLAGS": "-D rustdoc::broken_intra_doc_links"})
    # Overrides also work when previewing an older PR without these build helpers.
    config = json.loads((source / "web/typedoc.json").read_text())
    config.update(entryPoints=[str(source / "web/src")], tsconfig=str(source / "web/tsconfig.json"),
                  out=str(out / "typescript"), navigationLinks={
                      "All API documentation": base,
                      "TypeScript architecture": base + "typescript-architecture/",
                      "Rust architecture": base + "rust-architecture/",
                      "Documentation versions": SITE,
                  })
    with tempfile.TemporaryDirectory(prefix="procinsh-typedoc-") as directory:
        options = Path(directory) / "typedoc.json"
        options.write_text(json.dumps(config))
        run(["npm", "exec", "--", "typedoc", "--options", str(options)], source / "web")
    run(["npm", "--prefix", "web", "run", "docs:architecture"], source)
    graph = out / "rust-architecture"
    graph.mkdir(parents=True, exist_ok=True)
    with (graph / "modules-raw.dot").open("w") as output:
        run(["cargo", "modules", "dependencies", "--bin", "procinsh", "--no-externs", "--no-sysroot",
             "--no-fns", "--no-traits", "--no-types"], source, stdout=output)
    run(["python3", "scripts/module_dependencies.py", str(graph / "modules-raw.dot")], source)
    if key == "main":
        run(["npm", "--prefix", "web", "run", "build:replay"], source)
        shutil.copytree(source / "web/dist-replay", out / "demo")
    source_link = f"https://github.com/{repo}/tree/{sha}"
    label = "main" if key == "main" else f"PR #{metadata['number']}"
    groups = {"cargo doc": [], "module dependency": [], "TypeDoc": [], "Other": []}
    for directory in sorted(out.iterdir()):
        if directory.is_dir() and (directory / "index.html").is_file():
            name = directory.name
            if name in {"rust-architecture", "typescript-architecture"}:
                group = "module dependency"
            elif name == "typescript":
                group = "TypeDoc"
            elif (directory / "sidebar-items.js").is_file():
                group = "cargo doc"
            else:
                group = "Other"
            groups[group].append(f'<li><a href="{html.escape(name)}/">{html.escape(name)}</a></li>')
    groups["Other"].append(f'<li><a href="https://github.com/{repo}/blob/{sha}/DEVELOPMENT.md">Development guide</a></li>')
    sections = "".join(
        f'<h2>{heading}</h2><ul>' + "".join(links) + '</ul>'
        for heading, links in groups.items() if links
    )
    (out / "index.html").write_text(
        '<!doctype html><html lang="en"><meta charset="utf-8">'
        '<meta name="viewport" content="width=device-width,initial-scale=1">'
        f'<title>procinsh documentation — {label}</title><h1>procinsh documentation — {label}</h1>'
        f'<p>Commit <a href="{source_link}">{sha}</a></p>'
        f'<p><a href="{SITE}">All documentation versions</a></p>' + sections + '</html>')
    artifact.mkdir(parents=True, exist_ok=True)
    # Reject links before upload as well as at the publication boundary.
    if any(path.is_symlink() for path in out.rglob("*")):
        raise ValueError("Documentation cannot contain symbolic links")
    shutil.copytree(out, artifact / "site", dirs_exist_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    resolve_args = sub.add_parser("resolve")
    resolve_args.add_argument("--pr", default="")
    resolve_args.add_argument("--artifact", type=Path, required=True)
    build_args = sub.add_parser("build")
    build_args.add_argument("--source", type=Path, required=True)
    build_args.add_argument("--artifact", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "resolve":
        event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text())
        metadata = resolve(event, args.pr, os.environ["GITHUB_REPOSITORY"])
        args.artifact.mkdir(parents=True, exist_ok=True)
        (args.artifact / "metadata.json").write_text(json.dumps(metadata))
        with open(os.environ["GITHUB_OUTPUT"], "a") as output:
            for key in ["key", "sha"]:
                output.write(f"{key}={metadata[key]}\n")
    else:
        build(args.source, args.artifact, json.loads((args.artifact / "metadata.json").read_text()))


if __name__ == "__main__":
    main()
