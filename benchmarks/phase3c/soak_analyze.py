#!/usr/bin/env python3
"""Phase 3C memory plateau analysis for the soak results.

    python3 benchmarks/phase3c/soak_analyze.py RESULT.json [RESULT.json ...] [--late-hours 2] [--md OUT.md]

Accepts both formats: benchmarks/phase2g_soak.mjs (samples[].main /
samples[].i18n) and benchmarks/phase3c/velasia-soak.mjs (samples[].proc).

Phases (load samples split by time): warm-up (first 15 min, cache
population), steady (the rest of the load), late window (the last
--late-hours of load), idle (post-load samples). For each memory series:
mean and range per phase, least-squares slope (MiB/hour) and its 95 %
interval over the steady phase and the late window, and the coefficient of
variation in the late window.

"Plateau: yes" means the late-window slope's upper 95 % bound is below
1 MiB/hour (under 6 MiB over six hours), or its interval includes 0 with a
half-width under 5 MiB/hour. An interval that includes 0 only because it is
wide is "inconclusive"; one entirely above 1 MiB/hour is "no". This is an
evidence label, not a proof of boundedness.
"""
import json
import math
import sys


def series(doc):
    """{name: [(t_seconds, phase, value_mib)]} for every process in the document."""
    out = {}
    for s in doc.get("samples", []):
        t = s.get("t", 0)
        phase = s.get("phase", "load")
        procs = []
        if "proc" in s:
            procs.append(("velasia", s.get("proc") or {}, s))
        for key in ("main", "i18n"):
            if s.get(key):
                procs.append((key, s[key], s[key]))
        for name, p, m in procs:
            def add(metric, value):
                if value is None:
                    return
                out.setdefault(f"{name}.{metric}", []).append((t, phase, float(value)))
            add("rss", p.get("rssMiB"))
            add("anon", p.get("anonMiB"))
            add("swap", p.get("swapMiB"))
            mal = m.get("malloc")
            if mal:
                add("malloc_in_use", (mal["in_use_bytes"] + mal["mmap_bytes"]) / 1048576)
                add("malloc_free", mal["free_bytes"] / 1048576)
            heap = m.get("heapAfterGcMiB")
            if isinstance(heap, list):
                add("heap_after_gc", sum(heap))
            elif heap is not None:
                add("heap_after_gc", heap)
            used = m.get("heapUsedMiB")
            if isinstance(used, list):
                add("heap_used", sum(used))
            elif used is not None:
                add("heap_used", used)
            if m.get("cacheMemoryMiB") is not None:
                add("cache", m["cacheMemoryMiB"])
            elif isinstance(m.get("cache"), dict) and m["cache"].get("memory_bytes") is not None:
                add("cache", m["cache"]["memory_bytes"] / 1048576)
    return out


def slope(points):
    """Least-squares slope in MiB/hour with a 95 % interval (t ~ 1.96)."""
    n = len(points)
    if n < 3:
        return None
    xs = [p[0] / 3600 for p in points]
    ys = [p[2] for p in points]
    mx, my = sum(xs) / n, sum(ys) / n
    sxx = sum((x - mx) ** 2 for x in xs)
    if sxx == 0:
        return None
    b = sum((x - mx) * (y - my) for x, y in zip(xs, ys)) / sxx
    resid = [y - (my + b * (x - mx)) for x, y in zip(xs, ys)]
    se = math.sqrt(sum(r * r for r in resid) / (n - 2) / sxx) if n > 2 else 0
    return b, b - 1.96 * se, b + 1.96 * se


def stats(points):
    ys = [p[2] for p in points]
    if not ys:
        return None
    mean = sum(ys) / len(ys)
    sd = math.sqrt(sum((y - mean) ** 2 for y in ys) / len(ys))
    return {"n": len(ys), "mean": mean, "min": min(ys), "max": max(ys), "cv": sd / mean if mean else 0}


def analyze(path, late_hours):
    doc = json.load(open(path))
    rows = []
    for name, pts in sorted(series(doc).items()):
        load = [p for p in pts if p[1] != "idle"]
        idle = [p for p in pts if p[1] == "idle"]
        if not load:
            continue
        end = load[-1][0]
        warm = [p for p in load if p[0] < 900]
        steady = [p for p in load if p[0] >= 900]
        late = [p for p in load if p[0] >= end - late_hours * 3600]
        s_steady, s_late = slope(steady), slope(late)
        late_stats = stats(late)
        plateau = None
        if s_late:
            b, lo, hi = s_late
            if hi < 1.0:
                plateau = "yes"
            elif lo <= 0 <= hi:
                plateau = "yes" if (hi - lo) / 2 < 5.0 else "inconclusive"
            else:
                plateau = "no"
        rows.append({
            "series": name,
            "warmup": stats(warm), "steady": stats(steady), "late": late_stats, "idle": stats(idle),
            "steady_slope": s_steady, "late_slope": s_late, "plateau": plateau,
        })
    return doc, rows


def fmt(x, d=1):
    return "–" if x is None else f"{x:.{d}f}"


def main():
    args = sys.argv[1:]
    late_hours = 2.0
    md_out = None
    files = []
    i = 0
    while i < len(args):
        if args[i] == "--late-hours":
            late_hours = float(args[i + 1]); i += 2
        elif args[i] == "--md":
            md_out = args[i + 1]; i += 2
        else:
            files.append(args[i]); i += 1
    lines = []
    for path in files:
        doc, rows = analyze(path, late_hours)
        lines.append(f"## {path}\n")
        lines.append(f"Late window: last {late_hours:g} h of load. Slopes in MiB/hour (95 % interval).\n")
        lines.append("| Series | Warm-up mean | Steady mean (min–max) | Steady slope | Late slope | Late CV | Idle mean | Plateau |")
        lines.append("|---|---|---|---|---|---|---|---|")
        for r in rows:
            st, ls, ss = r["steady"], r["late_slope"], r["steady_slope"]
            lines.append(
                f"| {r['series']} | {fmt(r['warmup'] and r['warmup']['mean'])} | "
                f"{fmt(st and st['mean'])} ({fmt(st and st['min'])}–{fmt(st and st['max'])}) | "
                f"{fmt(ss and ss[0], 2)} [{fmt(ss and ss[1], 2)}, {fmt(ss and ss[2], 2)}] | "
                f"{fmt(ls and ls[0], 2)} [{fmt(ls and ls[1], 2)}, {fmt(ls and ls[2], 2)}] | "
                f"{fmt(r['late'] and r['late']['cv'] * 100, 2)} % | {fmt(r['idle'] and r['idle']['mean'])} | "
                f"{r['plateau'] or '–'} |"
            )
        lines.append("")
    text = "\n".join(lines)
    print(text)
    if md_out:
        open(md_out, "w").write(text)


if __name__ == "__main__":
    main()
