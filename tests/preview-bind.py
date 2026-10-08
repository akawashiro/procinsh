"""Check preview listener selection without Tailscale or a running server."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile


with tempfile.TemporaryDirectory() as directory:
    root = Path(directory)
    tailscale = root / "tailscale"
    tailscale.write_text(
        '#!/bin/bash\n'
        '[[ "$*" == "ip -4" ]] || exit 2\n'
        'printf "%s\\n" "$TEST_TAILSCALE_ADDRESS"\n'
        'exit "$TEST_TAILSCALE_STATUS"\n'
    )
    tailscale.chmod(0o755)
    binary = root / "binary"
    binary.write_text(
        f"#!{sys.executable}\nimport json, sys\nprint(json.dumps(sys.argv[1:]))\n"
    )
    binary.chmod(0o755)
    env = dict(os.environ, PATH=f"{directory}:{os.environ['PATH']}",
               TEST_TAILSCALE_ADDRESS="100.101.102.103", TEST_TAILSCALE_STATUS="0")

    def run(*args):
        return subprocess.run(
            ["/bin/bash", "scripts/preview_bind.sh", *args], env=env,
            capture_output=True, text=True, timeout=10,
        )

    result = run("address")
    assert result.returncode == 0, result.stderr
    assert result.stdout == "100.101.102.103\n", result.stdout
    for port in ("9090", "9091"):
        result = run("run", str(binary), port)
        assert result.returncode == 0, result.stderr
        assert json.loads(result.stdout) == [
            "--listen", f"100.101.102.103:{port}",
            "--listen", f"127.0.0.1:{port}",
            "--listen", f"[::1]:{port}", "--allow-non-loopback",
        ], result.stdout

    for address, status in (("", "0"), ("100.101.102.103", "1")):
        env.update(TEST_TAILSCALE_ADDRESS=address, TEST_TAILSCALE_STATUS=status)
        for args in (("address",), ("run", str(binary), "9090")):
            result = run(*args)
            assert result.returncode != 0
            assert result.stdout == "", result.stdout

print("Preview bind checks passed.")
