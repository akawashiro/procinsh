#!/usr/bin/env python3
"""Summarize leaf-only cpu-clock samples without attributing inlined/helper callers."""
from collections import Counter
import gzip
import json
from pathlib import Path
import re
import sys


def summarize(path):
    text = gzip.decompress(path.read_bytes()).decode() if path.suffix == ".gz" else path.read_text()
    dsos = Counter()
    symbols = Counter()
    for line in text.splitlines():
        if "cpu-clock:" not in line:
            continue
        left, dso = line.rsplit(" (", 1)
        dso = dso[:-1]
        symbol = re.split(r"cpu-clock:\s+[0-9a-f]+\s+", left)[1]
        dso = dso if dso.startswith("[") else Path(dso).name
        dsos[dso] += 1
        symbols[(dso, symbol)] += 1
    total = sum(dsos.values())
    assert total > 0
    return {
        "samples": total,
        "dsos": [{"dso": k, "samples": v, "percent": v * 100 / total} for k, v in dsos.most_common()],
        "symbols": [{"dso": d, "symbol": s, "samples": n, "percent": n * 100 / total}
                    for (d, s), n in symbols.most_common()],
    }


if __name__ == "__main__":
    source = Path(sys.argv[1])
    result = summarize(source)
    Path(sys.argv[2]).write_text(json.dumps(result, indent=2) + "\n")
    print(f"{result['samples']} samples")
    for row in result["dsos"]:
        print(f"{row['percent']:.2f}% {row['dso']}")
