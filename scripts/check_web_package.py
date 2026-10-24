#!/usr/bin/env python3
"""Check packaged assets, excluded tests, and builds without Node/npm."""

import os
import json
from pathlib import Path
import subprocess
import tempfile


def main():
    root = Path(__file__).resolve().parents[1]
    metadata = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--locked", "--no-deps", "--format-version", "1"],
        cwd=root, text=True,
    ))
    assert len(metadata["packages"]) == 1, "Keep one publishable Cargo package"
    package = metadata["packages"][0]
    assert package["default_run"] == "procinsh"
    assert {t["name"] for t in package["targets"] if t["kind"] == ["bin"]} == {
        "procinsh", "procinsh-privileged-helper"
    }
    artifacts = {
        path.relative_to(root).as_posix()
        for path in (root / "web/dist").rglob("*")
        if path.is_file()
    }
    assert artifacts, "Run npm --prefix web run build before checking the package"
    packaged = set(subprocess.check_output(
        ["cargo", "package", "--locked", "--allow-dirty", "--list"],
        cwd=root, text=True,
    ).splitlines())
    assert artifacts <= packaged, f"Missing frontend artifacts: {artifacts - packaged}"
    assert not any("node_modules/" in path for path in packaged)
    test_files = {path for path in packaged if path.startswith("tests/")}
    assert not test_files, f"Unexpected packaged test files: {sorted(test_files)}"
    # Cargo verifies the unpacked crate in a directory without Git metadata.
    # Fail if its build tries to use either frontend executable.
    with tempfile.TemporaryDirectory(prefix="procinsh-no-node-") as directory:
        for name in ["node", "npm", "npx"]:
            shim = Path(directory) / name
            shim.write_text("#!/bin/sh\necho 'Frontend tool invoked during Cargo build' >&2\nexit 1\n")
            shim.chmod(0o755)
        env = {**os.environ, "PATH": directory + os.pathsep + os.environ["PATH"]}
        subprocess.run(
            ["cargo", "package", "--locked", "--allow-dirty"],
            cwd=root, env=env, check=True,
        )
    print(f"Verified crate packaging and Node-free build ({len(artifacts)} frontend artifacts)")


if __name__ == "__main__":
    main()
