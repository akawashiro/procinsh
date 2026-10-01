#!/usr/bin/env python3
"""Format repository C sources, or check them without changes using --check."""
import argparse
import os
from pathlib import Path
import re
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="fail on formatting differences")
    args = parser.parse_args()
    root = Path(__file__).resolve().parent.parent
    requirement = (root / "requirements-format.txt").read_text().strip()
    expected = requirement.removeprefix("clang-format==")
    executable = os.environ.get("CLANG_FORMAT", "clang-format")
    try:
        output = subprocess.check_output([executable, "--version"], text=True)
    except (OSError, subprocess.CalledProcessError) as error:
        print(f"Cannot run {executable}: {error}. Install {requirement}.", file=sys.stderr)
        return 1
    version = re.search(r"\bversion (\d+\.\d+\.\d+)\b", output)
    if not version or version.group(1) != expected:
        print(f"Expected clang-format {expected}; got {output.strip()}", file=sys.stderr)
        return 1
    # Include newly added sources, but exclude ignored build outputs such as vmlinux.h.
    paths = subprocess.check_output(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z", "--", "*.c", "*.h"],
        cwd=root,
    ).split(b"\0")
    files = sorted({os.fsdecode(path) for path in paths if path and (root / os.fsdecode(path)).is_file()})
    if not files:
        return 0
    options = ["--dry-run", "--Werror"] if args.check else ["-i"]
    return subprocess.call([executable, "--style=file", *options, "--", *files], cwd=root)


if __name__ == "__main__":
    sys.exit(main())
