#!/usr/bin/env python3
"""Validate the four CPU intervals and reproduce both performance summaries."""
from collections import defaultdict
import json
from pathlib import Path
from statistics import median


def summarize(root):
    records = json.loads((root / "measurements.json").read_text())["records"]
    assert len(records) == 4
    cpu = {}
    for variant in ("before", "after"):
        runs = [r for r in records if r["variant"] == variant]
        assert len(runs) == 2
        for r in runs:
            elapsed = r["monotonic_end"] - r["monotonic_start"]
            expected = 100 * (r["user_seconds"] + r["system_seconds"]) / elapsed
            assert 59 <= elapsed <= 61 and abs(expected - r["cpu_percent"]) < 1e-6
        elapsed = sum(r["elapsed_seconds"] for r in runs)
        cpu[variant] = {
            "cpu_percent": 100 * sum(r["user_seconds"] + r["system_seconds"] for r in runs) / elapsed,
            "user_percent": 100 * sum(r["user_seconds"] for r in runs) / elapsed,
            "system_percent": 100 * sum(r["system_seconds"] for r in runs) / elapsed,
            "runs": [r["cpu_percent"] for r in runs],
        }
    cpu["relative_cpu_change_percent"] = 100 * (cpu["after"]["cpu_percent"] / cpu["before"]["cpu_percent"] - 1)
    (root / "cpu-summary.json").write_text(json.dumps(cpu, indent=2) + "\n")
    rows = [json.loads(line) for line in (root / "parser-bench.jsonl").read_text().splitlines()]
    groups = defaultdict(list)
    for row in rows[1:]:
        groups[row["variant"]].append(row["million_rows_per_cpu_second"])
    assert len(groups) == 4 and all(len(v) == 5 for v in groups.values())
    base = median(groups["unicode_vec"])
    replay = {
        "input": rows[0],
        "variants": {k: {"rounds": len(v), "median_million_rows_per_cpu_second": median(v),
                          "min": min(v), "max": max(v), "throughput_vs_baseline": median(v) / base}
                     for k, v in groups.items()},
    }
    (root / "parser-summary.json").write_text(json.dumps(replay, indent=2) + "\n")
    print(f"Backend CPU: {cpu['before']['cpu_percent']:.2f}% -> {cpu['after']['cpu_percent']:.2f}%")
    print(f"Relative CPU change: {cpu['relative_cpu_change_percent']:+.2f}%")


if __name__ == "__main__":
    summarize(Path(__file__).parent)
