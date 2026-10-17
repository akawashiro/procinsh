#!/usr/bin/env python3
"""Validate a complete comparison and summarize process CPU time per build profile."""
import json
from pathlib import Path
import sys


def summarize(path):
    data = json.loads(path.read_text())
    rows = data["records"]
    assert len(rows) == 8, "Expected two runs of each variant for debug and release"
    result = {}
    for profile in ("debug", "release"):
        result[profile] = {}
        for variant in ("before", "after"):
            runs = [r for r in rows if r["profile"] == profile and r["variant"] == variant]
            assert len(runs) == 2
            for run in runs:
                duration = run["monotonic_end"] - run["monotonic_start"]
                assert 59 <= duration <= 61
                expected = 100 * (run["user_seconds"] + run["system_seconds"]) / duration
                assert abs(run["cpu_percent"] - expected) < 1e-6
                assert not run["after"]["errors"]
                assert run["after"]["counts"]["snapshot"] > run["before"]["counts"]["snapshot"]
                assert run["after"]["counts"]["activity"] > run["before"]["counts"]["activity"]
                assert run["workloadBefore"]["tabs"] == run["workloadAfter"]["tabs"] == 20
            elapsed = sum(r["elapsed_seconds"] for r in runs)
            user = 100 * sum(r["user_seconds"] for r in runs) / elapsed
            system = 100 * sum(r["system_seconds"] for r in runs) / elapsed
            result[profile][variant] = {
                "runs": len(runs),
                "cpu_percent": user + system,
                "user_percent": user,
                "system_percent": system,
                "run_cpu_percent": [r["cpu_percent"] for r in runs],
                "maps_at_start": [r["before"]["snapshot"]["maps"] for r in runs],
                "maps_at_end": [r["after"]["snapshot"]["maps"] for r in runs],
            }
        before = result[profile]["before"]["cpu_percent"]
        after = result[profile]["after"]["cpu_percent"]
        result[profile]["cpu_change_percentage_points"] = after - before
        result[profile]["relative_cpu_change_percent"] = 100 * (after / before - 1)
    return result


if __name__ == "__main__":
    source = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).with_name("measurements.json")
    summary = summarize(source)
    source.with_name("summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print("| Profile | Before CPU | After CPU | Relative CPU change |")
    print("|---|---:|---:|---:|")
    for profile, row in summary.items():
        print(f"| {profile} | {row['before']['cpu_percent']:.2f}% | "
              f"{row['after']['cpu_percent']:.2f}% | "
              f"{row['relative_cpu_change_percent']:+.2f}% |")
