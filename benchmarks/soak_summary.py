#!/usr/bin/env python3
"""Summarizes soak.mjs results: memory trends and request outcomes.

    python3 benchmarks/soak_summary.py soak-i1.json [soak-i2.json ...] \
        [--warmup-minutes 10] [--json out.json]

Per file: duration, requests, throughput, restarts, failures, and for RSS,
anonymous RSS, V8 heap after full GC, heap reserved and heap used: first
and last value after warm-up, min, max, and the least-squares slope in
MiB per hour after warm-up. Heap used saw-tooths between collections, so
it is reported but not used to judge leaks; the heap after full GC is the
live-data floor. A slope is only meaningful relative to the run's
duration and noise (see the residual standard deviation).
"""

import argparse
import json
import statistics
import sys
from pathlib import Path

SERIES = [
    ("rssMiB", "RSS"),
    ("rssAnonMiB", "anonymous RSS"),
    ("swapMiB", "server memory in swap"),
    ("heapAfterGcMiB", "heap after full GC"),
    ("heapTotalMiB", "heap reserved"),
    ("heapUsedMiB", "heap used"),
    ("cacheMemoryMiB", "data cache in memory"),
    ("cacheStorageMiB", "data cache on disk"),
]


def slope(points):
    """Least squares (hours, value) -> (MiB/hour, residual stdev)."""
    if len(points) < 3:
        return None, None
    xs = [p[0] for p in points]
    ys = [p[1] for p in points]
    mx, my = statistics.fmean(xs), statistics.fmean(ys)
    sxx = sum((x - mx) ** 2 for x in xs)
    if sxx == 0:
        return None, None
    b = sum((x - mx) * (y - my) for x, y in zip(xs, ys)) / sxx
    a = my - b * mx
    residuals = [y - (a + b * x) for x, y in zip(xs, ys)]
    return b, statistics.pstdev(residuals)


def summarize(path, warmup_minutes):
    data = json.loads(Path(path).read_text())
    samples = data["samples"]
    duration_s = samples[-1]["t"]
    counts = data["counts"]
    failures = {k: v for k, v in counts.items() if k.endswith("-FAILED")}
    out = {
        "file": Path(path).name,
        "duration_minutes": round(duration_s / 60, 1),
        "concurrency": data["concurrency"],
        "completed": data["completed"],
        "requests_per_second": round(data["completed"] / max(duration_s, 1), 1),
        "restarts": samples[-1].get("restarts"),
        "injections": len(data.get("injections", [])),
        "lost_to_restart": counts.get("lost-to-restart", 0),
        "failures": failures,
        "handler_errors": samples[-1].get("errors5xx"),
        "rejected": samples[-1].get("rejected"),
        "series": {},
    }
    # After warm-up, during the load (the last sample is taken after it),
    # and not while an isolate restarts (its heap gauges read 0 then).
    isolates = max((s.get("ready") or 0) for s in samples) or 1
    steady = [
        s
        for s in samples[:-1]
        if s["t"] >= warmup_minutes * 60 and (s.get("ready") or 0) >= isolates
        and (s.get("heapAfterGcMiB") is None or s["heapAfterGcMiB"] > 0)
    ]
    for key, label in SERIES:
        points = [(s["t"] / 3600, s[key]) for s in steady if s.get(key) is not None]
        if len(points) < 3:
            continue
        values = [p[1] for p in points]
        b, noise = slope(points)
        buckets = {}
        for hours, value in points:
            buckets.setdefault(int(hours * 6), []).append(value)
        out["series"].setdefault(key, {})
        out["series"][key] = {
            "ten_minute_medians": [round(statistics.median(v), 1) for _, v in sorted(buckets.items())],
            "label": label,
            "first": values[0],
            "last": values[-1],
            "min": min(values),
            "max": max(values),
            "slope_mib_per_hour": round(b, 2) if b is not None else None,
            "residual_stdev_mib": round(noise, 2) if noise is not None else None,
        }
    return out


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("files", nargs="+")
    parser.add_argument("--warmup-minutes", type=float, default=10)
    parser.add_argument("--json")
    args = parser.parse_args()
    results = [summarize(f, args.warmup_minutes) for f in args.files]
    for r in results:
        print(
            f"{r['file']}: {r['duration_minutes']} min, {r['completed']:,} requests "
            f"({r['requests_per_second']}/s), restarts {r['restarts']} of {r['injections']} injections, "
            f"lost to restart {r['lost_to_restart']}, failures {r['failures'] or 'none'}, "
            f"handler errors {r['handler_errors']}, rejected {r['rejected']}"
        )
        for key, s in r["series"].items():
            print(
                f"  {s['label']:22} first {s['first']:7.1f}  last {s['last']:7.1f}  "
                f"min {s['min']:7.1f}  max {s['max']:7.1f}  slope {s['slope_mib_per_hour']:+7.2f} MiB/h  "
                f"(residual sd {s['residual_stdev_mib']})"
            )
            print(f"  {'':22} 10-min medians: {s['ten_minute_medians']}")
    if args.json:
        Path(args.json).write_text(json.dumps(results, indent=2))
    sys.exit(0)


if __name__ == "__main__":
    main()
