#!/usr/bin/env python3
"""Phase 1C benchmarks: outbound fetch and the shared data cache, against
`next start` and the Phase 1B binary (stdlib only; load: ab and
benchmarks/cold_bench.mjs).

    python3 benchmarks/phase1c_bench.py --project fixtures/next-app \
        --nextrs-1b /path/to/phase-1b/nextrs --out benchmarks/results

Servers, all on the same .next build and one upstream
(tests/compat/upstream.mjs):

  next-start        `next start`
  nextrs-1b         the Phase 1B binary (no network: data paths skipped)
  1c-off            this tree, network off, --no-shared-cache (Phase 1B's
                    configuration)
  1c-net            network allowed, --no-shared-cache (Next's per-isolate
                    default cache)
  1c-cache          network allowed, shared cache in memory (the default)
  1c-cache-disk     network allowed, shared cache with --cache-dir
  1b-i4, 1c-cache-i4  the same with 4 isolates

Paths:
  /books/42, /api/health      SSR and a trivial handler: regression check
  /data-fetch                 render with an uncached upstream fetch
  /api/data?...&cache=no-store       uncached fetch in a route handler
  /api/data?...&cache=force-cache    the same key every time: warm hits
  cold (cold_bench.mjs)       a new key per request: miss + fetch + store

Every server must answer /books/42 byte-identically to next start before
it is measured. Results go to <out>/phase1c-<time>.json.
"""

import argparse
import json
import os
import shutil
import socket
import statistics
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from bench import ROOT, environment, tree_cpu_seconds, tree_rss_kib  # noqa: E402
from dynamic_bench import HEADERS, Server, commands, fetch  # noqa: E402

UPSTREAM_PORT = 4955
CLOSED_PORT = 4956
SSR = ["/books/42", "/api/health"]
UNCACHED_FETCH = ["/data-fetch", "/api/data?key=bench-nostore&cache=no-store"]
WARM = "/api/data?key=bench-warm&cache=force-cache"


def run_ab(url, requests, concurrency):
    """ab, counting only real failures: ab also counts responses whose
    length differs from the first, which the data paths' hit counters do."""
    import csv
    import io
    import re

    with tempfile.NamedTemporaryFile("r", suffix=".csv") as percentiles:
        cmd = ["ab", "-q", "-k", "-n", str(requests), "-c", str(concurrency), "-e", percentiles.name]
        for name, value in HEADERS.items():
            cmd += ["-H", f"{name}: {value}"]
        out = subprocess.run(cmd + [url], check=True, capture_output=True, text=True).stdout
        rows = list(csv.reader(io.StringIO(Path(percentiles.name).read_text())))[1:]
    served = {int(float(p)): float(ms) for p, ms in rows}
    breakdown = re.search(r"Connect: (\d+), Receive: (\d+), Length: \d+, Exceptions: (\d+)", out)
    failed = sum(int(n) for n in breakdown.groups()) if breakdown else 0
    non2xx = re.search(r"Non-2xx responses:\s+(\d+)", out)
    return {
        "requests_per_second": float(re.search(r"Requests per second:\s+([\d.]+)", out).group(1)),
        "failed": failed + (int(non2xx.group(1)) if non2xx else 0),
        "p50_ms": served.get(50),
        "p95_ms": served.get(95),
        "p99_ms": served.get(99),
    }


def metrics(port):
    if not port:
        return None
    with urllib.request.urlopen(f"http://127.0.0.1:{port}/metrics.json") as response:
        return json.loads(response.read())


def free(port):
    with socket.socket() as s:
        return s.connect_ex(("127.0.0.1", port)) != 0


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--project", default="fixtures/next-app")
    parser.add_argument("--out", default="benchmarks/results")
    parser.add_argument("--nextrs-1b", default=None)
    parser.add_argument("--concurrency", type=int, nargs="+", default=[32, 128])
    parser.add_argument("--requests", type=int, default=4000)
    parser.add_argument("--repeats", type=int, default=2)
    parser.add_argument("--startup-runs", type=int, default=3)
    parser.add_argument("--only", nargs="*", default=None)
    args = parser.parse_args()

    project = (ROOT / args.project).resolve()
    upstream = f"http://127.0.0.1:{UPSTREAM_PORT}"
    base = commands(project, 0, None)
    next_cmd, next_cwd, next_env = base["next-start"]
    next_env = {**next_env, "NEXTRS_UPSTREAM": upstream}
    rs_cmd, rs_cwd, rs_env = base["nextrs-v8"]
    rs_env = {**rs_env, "NEXTRS_UPSTREAM": upstream}
    net = [
        "--js-env", "NEXTRS_UPSTREAM",
        "--net-allow", upstream, "--net-allow", f"http://127.0.0.1:{CLOSED_PORT}",
        "--net-allow-addr", "127.0.0.1",
    ]
    cache_dir = Path(tempfile.mkdtemp(prefix="nextrs-bench-cache-"))
    variants = {"next-start": ((next_cmd, next_cwd, next_env), None, True)}
    if args.nextrs_1b:
        variants["nextrs-1b"] = (([args.nextrs_1b] + rs_cmd[1:], rs_cwd, rs_env), 9301, False)
        variants["1b-i4"] = (([args.nextrs_1b] + rs_cmd[1:] + ["--isolates", "4"], rs_cwd, rs_env), 9302, False)
    variants["1c-off"] = ((rs_cmd + ["--no-shared-cache"], rs_cwd, rs_env), 9303, False)
    variants["1c-net"] = ((rs_cmd + net + ["--no-shared-cache"], rs_cwd, rs_env), 9304, True)
    variants["1c-cache"] = ((rs_cmd + net, rs_cwd, rs_env), 9305, True)
    variants["1c-cache-disk"] = ((rs_cmd + net + ["--cache-dir", str(cache_dir)], rs_cwd, rs_env), 9306, True)
    variants["1c-cache-i4"] = ((rs_cmd + net + ["--isolates", "4"], rs_cwd, rs_env), 9307, True)
    if args.only:
        variants = {k: v for k, v in variants.items() if k in args.only}

    def make(name, port):
        (cmd, cwd, env), metrics_port, _ = variants[name]
        cmd = cmd[:]
        for flag in ("-p", "--port"):
            if flag in cmd:
                cmd[cmd.index(flag) + 1] = str(port)
        if metrics_port:
            cmd += ["--metrics-addr", f"127.0.0.1:{metrics_port}"]
        return Server(name, cmd, cwd, env, port)

    if not free(UPSTREAM_PORT):
        sys.exit(f"port {UPSTREAM_PORT} is in use")
    upstream_proc = subprocess.Popen(
        ["node", str(ROOT / "tests/compat/upstream.mjs"), "--port", str(UPSTREAM_PORT)],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True,
    )
    time.sleep(0.5)

    ports = {name: 3700 + i for i, name in enumerate(variants)}
    results = {"environment": environment(), "config": {**vars(args), "nextrs_1b": Path(args.nextrs_1b).name if args.nextrs_1b else None}, "startup": {}, "load": []}
    reference = None
    try:
        for name in variants:
            ready, first, idle = [], [], []
            for _ in range(args.startup_runs):
                shutil.rmtree(cache_dir, ignore_errors=True)
                server = make(name, ports[name])
                try:
                    started = time.perf_counter()
                    ready.append(server.start())
                    fetch(server.port, "/books/42")
                    first.append(time.perf_counter() - started)
                    time.sleep(1.0)
                    idle.append(tree_rss_kib(server.proc.pid))
                finally:
                    server.stop()
            results["startup"][name] = {
                "ready_ms": statistics.median(ready) * 1000,
                "first_dynamic_response_ms": statistics.median(first) * 1000,
                "rss_after_first_render_kib": statistics.median(idle),
            }
            print(f"{name:14} ready {statistics.median(ready) * 1000:7.1f} ms  first dynamic {statistics.median(first) * 1000:7.1f} ms  RSS {statistics.median(idle) / 1024:6.1f} MiB", flush=True)

        for name, (_, metrics_port, has_net) in variants.items():
            shutil.rmtree(cache_dir, ignore_errors=True)
            server = make(name, ports[name])
            try:
                server.start()
                status, body = fetch(server.port, "/books/42")
                if reference is None:
                    reference = body
                if status != 200 or body != reference:
                    sys.exit(f"{name}: /books/42 differs from next start; refusing to benchmark")
                pid = server.proc.pid
                plan = [(p, c) for p in SSR for c in args.concurrency]
                if has_net:
                    plan += [(p, c) for p in UNCACHED_FETCH + [WARM] for c in args.concurrency]
                    for path in UNCACHED_FETCH + [WARM]:
                        status, _ = fetch(server.port, path)
                        if status != 200:
                            sys.exit(f"{name} answered {status} for {path}")
                for path, concurrency in plan:
                    url = f"http://127.0.0.1:{server.port}{path}"
                    run_ab(url, max(args.requests // 8, 2 * concurrency), concurrency)
                    before = metrics(metrics_port)
                    runs = []
                    for _ in range(args.repeats):
                        cpu = tree_cpu_seconds(pid)
                        measured = run_ab(url, args.requests, concurrency)
                        measured["server_cpu_ms_per_request"] = (tree_cpu_seconds(pid) - cpu) / args.requests * 1000
                        measured["rss_kib_after"] = tree_rss_kib(pid)
                        runs.append(measured)
                    summary = {k: statistics.median(r[k] for r in runs) for k in ("requests_per_second", "p50_ms", "p95_ms", "p99_ms", "server_cpu_ms_per_request")}
                    summary["rss_mib_after"] = runs[-1]["rss_kib_after"] / 1024
                    summary["failed"] = sum(r["failed"] for r in runs)
                    after = metrics(metrics_port)
                    if before and after:
                        if after.get("cache"):
                            hits = after["cache"]["hits"] - before["cache"]["hits"]
                            misses = after["cache"]["misses"] - before["cache"]["misses"]
                            summary["cache_hit_ratio"] = hits / max(hits + misses, 1)
                        fetched = after["fetch"]["duration_count"] - before["fetch"]["duration_count"]
                        if fetched:
                            summary["fetch_time_to_head_ms"] = (after["fetch"]["duration_seconds_sum"] - before["fetch"]["duration_seconds_sum"]) / fetched * 1000
                    results["load"].append({"server": name, "path": path, "concurrency": concurrency, "median": summary, "runs": runs})
                    extra = "".join(f"  {k} {summary[k]:.3f}" for k in ("cache_hit_ratio", "fetch_time_to_head_ms") if k in summary)
                    print(f"{path[:44]:44} c={concurrency:<3} {name:14} {summary['requests_per_second']:8.0f} req/s  p50 {summary['p50_ms']:7.2f}  p95 {summary['p95_ms']:7.2f}  p99 {summary['p99_ms']:7.2f} ms  cpu {summary['server_cpu_ms_per_request']:5.2f} ms/req  rss {summary['rss_mib_after']:6.1f} MiB  failed {summary['failed']}{extra}", flush=True)
                if has_net:
                    for concurrency in args.concurrency:
                        before = metrics(metrics_port)
                        cpu = tree_cpu_seconds(pid)
                        out = subprocess.run(
                            ["node", str(ROOT / "benchmarks/cold_bench.mjs"), "--target", f"http://127.0.0.1:{server.port}", "--requests", str(args.requests // 2), "--concurrency", str(concurrency)],
                            check=True, capture_output=True, text=True,
                        ).stdout
                        summary = json.loads(out)
                        summary["server_cpu_ms_per_request"] = (tree_cpu_seconds(pid) - cpu) / (args.requests // 2) * 1000
                        summary["rss_mib_after"] = tree_rss_kib(pid) / 1024
                        after = metrics(metrics_port)
                        if before and after and after.get("cache"):
                            hits = after["cache"]["hits"] - before["cache"]["hits"]
                            misses = after["cache"]["misses"] - before["cache"]["misses"]
                            summary["cache_hit_ratio"] = hits / max(hits + misses, 1)
                            summary["cache_entries_after"] = after["cache"]["entries"]
                            summary["cache_storage_bytes_after"] = after["cache"]["storage_bytes"]
                        results["load"].append({"server": name, "path": "cold", "concurrency": concurrency, "median": summary})
                        print(f"{'cold (new key per request)':44} c={concurrency:<3} {name:14} {summary['requests_per_second']:8.0f} req/s  p50 {summary['p50_ms']:7.2f}  p95 {summary['p95_ms']:7.2f}  p99 {summary['p99_ms']:7.2f} ms  cpu {summary['server_cpu_ms_per_request']:5.2f} ms/req  rss {summary['rss_mib_after']:6.1f} MiB  failed {summary['failed']}", flush=True)
            finally:
                server.stop()
    finally:
        os.killpg(upstream_proc.pid, 15)
        shutil.rmtree(cache_dir, ignore_errors=True)

    out = ROOT / args.out
    out.mkdir(parents=True, exist_ok=True)
    path = out / f"phase1c-{time.strftime('%Y%m%dT%H%M%S')}.json"
    path.write_text(json.dumps(results, indent=2))
    print(f"wrote {path}")


if __name__ == "__main__":
    main()
