#!/usr/bin/env python3
"""Summarizes benchmarks/image_soak.mjs output: totals, per-class latency,
memory after warm-up (RSS, RSS + swap, malloc, V8 heap after GC) with
least-squares slopes, CPU, cache accounting.

    python3 benchmarks/image_soak_summary.py SOAK.json [--warmup-min 10]
"""
import json
import sys


def slope(points):
    """MiB per hour from (seconds, MiB) pairs."""
    n = len(points)
    if n < 2:
        return 0.0
    mx = sum(p[0] for p in points) / n
    my = sum(p[1] for p in points) / n
    den = sum((p[0] - mx) ** 2 for p in points)
    return 0.0 if den == 0 else sum((p[0] - mx) * (p[1] - my) for p in points) / den * 3600


def main():
    path = sys.argv[1]
    warmup = 10
    if "--warmup-min" in sys.argv:
        warmup = float(sys.argv[sys.argv.index("--warmup-min") + 1])
    d = json.load(open(path))
    s = [x for x in d["samples"] if x.get("images")]
    warm = [x for x in s if x["t"] >= warmup * 60]
    t = d["totals"]
    print(f"{path}: {d['seconds'] / 60:.1f} min, {t['requests']:,} requests, {d['throughput']} req/s, concurrency {d['concurrency']}")
    print(f"  unexpected {t['unexpected']}, identity leaks {t['identityLeaks']}, image 503 {t['rejected503']}, "
          f"lost to restart {t['lostToRestart']}, shed during restart {t.get('rejectedDuringRestart', 0)}, "
          f"client aborts {t['aborted']}, injections {t['injections']}")
    print(f"  latency all p50/p95/p99 ms: {d['latency']['p50']}/{d['latency']['p95']}/{d['latency']['p99']}")
    for k, v in d["kinds"].items():
        cache = v.get("cache") or {}
        hits = cache.get("HIT", 0)
        total = sum(cache.values())
        rate = f" hit rate {hits / total:.1%} ({cache})" if total else ""
        print(f"    {k:18} {v['count']:>9,} err {v['errors']:<3} p50 {v['p50']} p95 {v['p95']} p99 {v['p99']} statuses {v['statuses']}{rate}")

    def series(fn):
        return [(x["t"], fn(x)) for x in warm if fn(x) is not None]

    rss = series(lambda x: x["rssMiB"])
    rss_swap = series(lambda x: x["rssMiB"] + x["swapMiB"])
    malloc = series(lambda x: (x["malloc"]["in_use_bytes"] + x["malloc"]["mmap_bytes"]) / 1048576 if x.get("malloc") else None)
    # 0 while an isolate restarts (not sampled yet): skipped.
    heap = series(lambda x: max(x["heapAfterGcMiB"]) if x.get("heapAfterGcMiB") and max(x["heapAfterGcMiB"]) > 0 else None)
    def fmt(name, pts):
        vals = [p[1] for p in pts]
        if not vals:
            return f"  {name}: n/a"
        return f"  {name}: min {min(vals):.0f} max {max(vals):.0f} end {vals[-1]:.0f} MiB, slope {slope(pts):+.1f} MiB/h"
    print(f"  after {warmup:g} min warm-up ({len(warm)} samples):")
    print(f"  RSS at start {s[0]['rssMiB']:.0f} MiB, peak (VmHWM) {s[-1]['hwmMiB']:.0f} MiB, swap max {max(x['swapMiB'] for x in s):.0f} MiB")
    for name, pts in [("RSS", rss), ("RSS + swap", rss_swap), ("malloc in use", malloc), ("V8 heap after GC (max isolate)", heap)]:
        print(fmt(name, pts))
    cpu = (s[-1]["cpuSeconds"] - s[0]["cpuSeconds"]) / max(1, s[-1]["t"] - s[0]["t"])
    print(f"  CPU {cpu:.2f} cores on average; host load {min(x['load'] for x in s):.1f}-{max(x['load'] for x in s):.1f}")
    i0, i1 = s[0]["images"], s[-1]["images"]
    diff = {k: (i1.get(k) or 0) - (i0.get(k) or 0) for k in i1}
    hits = diff.get("cache_memory_hits", 0) + diff.get("cache_disk_hits", 0)
    lookups = hits + diff.get("cache_misses", 0)
    print(f"  images: transforms {diff['transforms']:,}, failures {diff['transform_failures']}, fallbacks {diff['fallbacks']}, "
          f"overloaded {diff['overloaded']}, cancelled {diff['cancelled']}, coalesced {diff['coalesced']}, "
          f"memory waits {diff['memory_waits']}, memory rejected {diff['memory_rejected']}, source rejected {diff['source_rejected']}")
    if lookups:
        print(f"  cache: {hits:,} hits / {lookups:,} lookups ({hits / lookups:.1%}), memory {i1['cache_memory_bytes'] / 1048576:.1f} MiB, "
              f"disk {i1['cache_disk_bytes'] / 1048576:.1f} MiB, write errors {diff['cache_write_errors']}, corrupt {diff['cache_corrupt']}")
    peak_reserved = max((x["images"].get("memory_reserved_bytes") or 0) for x in s)
    print(f"  reservations: peak sampled {peak_reserved / 1048576:.0f} MiB, at end {i1['memory_reserved_bytes']}, "
          f"source held at end {i1['source_held_bytes']}, queued {i1['queued']}, in flight {i1['in_flight']}; restarts {s[-1].get('restarts')}")
    if d["errors"]:
        print("  first errors:", json.dumps(d["errors"][:5])[:800])


if __name__ == "__main__":
    main()
