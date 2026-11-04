#!/usr/bin/env python3
"""Phase 1B benchmarks: the isolate pool against the single isolate, the
Phase 1A executor and `next start` (stdlib only; load generator: ab).

    python3 benchmarks/pool_bench.py --project fixtures/next-app \
        --nextrs-1a /path/to/phase-1a/nextrs [--isolates 1 2 4] \
        [--concurrency 1 8 32 64 128] --out benchmarks/results

Servers, all on the same .next build:

  next-start      `next start`
  nextrs-1a       the Phase 1A binary (single isolate, unbounded response
                  channel), if --nextrs-1a is given
  nextrs-1b-iN    this tree's target/release/nextrs with --isolates N

The equivalence gate of dynamic_bench.py applies before any measurement.
Per server and path/concurrency it records throughput, latency
percentiles, server CPU per request and RSS, plus startup, first dynamic
response and idle RSS. The nextrs-1b runs also record the executor's
queue-wait and execution totals from its metrics endpoint.
"""

import argparse
import json
import statistics
import sys
import time
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from bench import ROOT, environment, tree_cpu_seconds, tree_rss_kib  # noqa: E402
from dynamic_bench import Server, commands, fetch, run_ab  # noqa: E402

SSR_PATHS = ["/books/42", "/api/health"]
# /dynamic waits ~60 ms on timers per request; measured at higher
# concurrency only.
STREAM_PATH = "/dynamic"


def metrics(port):
    with urllib.request.urlopen(f"http://127.0.0.1:{port}/metrics.json") as response:
        return json.load(response)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--project", default="fixtures/next-app")
    parser.add_argument("--out", default="benchmarks/results")
    parser.add_argument("--nextrs-1a", default=None)
    parser.add_argument("--isolates", type=int, nargs="+", default=[1, 2, 4])
    parser.add_argument("--concurrency", type=int, nargs="+", default=[1, 8, 32, 64, 128])
    parser.add_argument("--stream-concurrency", type=int, nargs="+", default=[32, 128])
    parser.add_argument("--requests", type=int, default=3000)
    parser.add_argument("--repeats", type=int, default=2)
    parser.add_argument("--startup-runs", type=int, default=3)
    args = parser.parse_args()

    project = (ROOT / args.project).resolve()
    base = commands(project, 0, None)
    variants = {"next-start": (base["next-start"], None)}
    if args.nextrs_1a:
        cmd, cwd, env = base["nextrs-v8"]
        variants["nextrs-1a"] = (([args.nextrs_1a] + cmd[1:], cwd, env), None)
    for n in args.isolates:
        cmd, cwd, env = base["nextrs-v8"]
        variants[f"nextrs-1b-i{n}"] = ((cmd + ["--isolates", str(n)], cwd, env), 9200 + n)

    def make(name, port):
        (cmd, cwd, env), metrics_port = variants[name]
        cmd = cmd[:]
        # commands() was built for port 0; patch the port arguments.
        for flag in ("-p", "--port"):
            if flag in cmd:
                cmd[cmd.index(flag) + 1] = str(port)
        if metrics_port:
            cmd += ["--metrics-addr", f"127.0.0.1:{metrics_port}"]
        return Server(name, cmd, cwd, env, port)

    ports = {name: 3600 + i for i, name in enumerate(variants)}
    config = dict(vars(args))
    if config["nextrs_1a"]:
        # Record the file name only: committed results carry no local paths.
        config["nextrs_1a"] = Path(config["nextrs_1a"]).name
    results = {"environment": environment(), "config": config, "startup": {}, "load": []}

    for name in variants:
        ready, first, idle = [], [], []
        for _ in range(args.startup_runs):
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
        print(
            f"{name:15} ready {statistics.median(ready) * 1000:7.1f} ms  first dynamic "
            f"{statistics.median(first) * 1000:7.1f} ms  RSS {statistics.median(idle) / 1024:6.1f} MiB",
            flush=True,
        )

    # One server at a time, so they never compete for the CPU.
    plan = [(p, c) for p in SSR_PATHS for c in args.concurrency]
    plan += [(STREAM_PATH, c) for c in args.stream_concurrency]
    for name in variants:
        server = make(name, ports[name])
        try:
            server.start()
            for path in SSR_PATHS + [STREAM_PATH]:
                status, body = fetch(server.port, path)
                reference = fetch_reference(project, path)
                if status != 200:
                    sys.exit(f"{name} answered {status} for {path}; refusing to benchmark")
                if path == STREAM_PATH:
                    if b"Streamed after Suspense boundary" not in body:
                        sys.exit(f"{name} rendered an incomplete {path}")
                elif reference is not None and body != reference:
                    sys.exit(f"{name} body for {path} differs from next start; refusing to benchmark")
            pid = server.proc.pid
            for path, concurrency in plan:
                url = f"http://127.0.0.1:{server.port}{path}"
                requests = args.requests // 4 if path == STREAM_PATH else args.requests
                run_ab(url, max(requests // 8, 2 * concurrency, 50), concurrency)
                metrics_port = variants[name][1]
                before = metrics(metrics_port) if metrics_port else None
                runs = []
                for _ in range(args.repeats):
                    cpu = tree_cpu_seconds(pid)
                    measured = run_ab(url, requests, concurrency)
                    measured["requests"] = requests
                    measured["server_cpu_ms_per_request"] = (tree_cpu_seconds(pid) - cpu) / requests * 1000
                    measured["rss_kib_after"] = tree_rss_kib(pid)
                    runs.append(measured)
                summary = {
                    key: statistics.median(r[key] for r in runs)
                    for key in ("requests_per_second", "p50_ms", "p95_ms", "p99_ms", "server_cpu_ms_per_request")
                }
                summary["rss_mib_after"] = runs[-1]["rss_kib_after"] / 1024
                summary["failed"] = sum(r["failed"] for r in runs)
                if before is not None:
                    after = metrics(metrics_port)
                    waits = after["queue_waits"] - before["queue_waits"]
                    done = after["requests_completed"] - before["requests_completed"]
                    summary["queued_fraction"] = waits / max(done, 1)
                    summary["mean_queue_wait_ms"] = (
                        (after["queue_wait_seconds_total"] - before["queue_wait_seconds_total"]) / max(waits, 1) * 1000
                    )
                    summary["mean_execution_ms"] = (
                        (after["execution_seconds_total"] - before["execution_seconds_total"]) / max(done, 1) * 1000
                    )
                    summary["rejected"] = sum(
                        after[k] - before[k]
                        for k in ("rejected_overloaded", "rejected_queue_timeout", "rejected_unavailable")
                    )
                results["load"].append({"server": name, "path": path, "concurrency": concurrency, "median": summary, "runs": runs})
                extra = ""
                if "mean_execution_ms" in summary:
                    extra = f"  exec {summary['mean_execution_ms']:6.2f} ms  queued {summary['queued_fraction']:.2f}"
                print(
                    f"{path:12} c={concurrency:<3} {name:15} {summary['requests_per_second']:8.0f} req/s  "
                    f"p50 {summary['p50_ms']:7.2f}  p95 {summary['p95_ms']:7.2f}  p99 {summary['p99_ms']:7.2f} ms  "
                    f"cpu {summary['server_cpu_ms_per_request']:5.2f} ms/req  rss {summary['rss_mib_after']:6.1f} MiB  "
                    f"failed {summary['failed']}{extra}",
                    flush=True,
                )
        finally:
            server.stop()

    out = ROOT / args.out
    out.mkdir(parents=True, exist_ok=True)
    path = out / f"pool-{time.strftime('%Y%m%dT%H%M%S')}.json"
    path.write_text(json.dumps(results, indent=2))
    print(f"wrote {path}")


_reference = {}


def fetch_reference(project, path):
    """next start's body for `path`, fetched once from a temporary server."""
    if not _reference:
        cmd, cwd, env = commands(project, 3599, None)["next-start"]
        server = Server("reference", cmd, cwd, env, 3599)
        try:
            server.start()
            for p in SSR_PATHS:
                _reference[p] = fetch(3599, p)[1]
        finally:
            server.stop()
    return _reference.get(path)


if __name__ == "__main__":
    main()
