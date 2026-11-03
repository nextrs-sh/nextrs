#!/usr/bin/env python3
"""Phase 1A dynamic-rendering benchmarks (stdlib only; load generator: ab).

    python3 benchmarks/dynamic_bench.py --project fixtures/next-app \
        [--deno /path/to/deno] --out benchmarks/results

Every server renders the same routes from the same .next build by calling
Next's compiled route handlers:

  next-start     `next start` (Node.js; router-server + handler)
  nextrs-v8      `nextrs start --js-runtime embedded-v8` (Rust HTTP + V8 isolate)
  node-harness   experiments/route-handler-harness on Node.js (handler only)
  deno-harness   the same harness on the Deno CLI (reference only, optional)

Equivalence gate: before measuring a path, its body is fetched from every
server. next-start, nextrs-v8 and node-harness must be byte-identical
(per-request random keys aside, which these paths do not contain).
deno-harness is only checked for status 200, because its HTML is
semantically equal but chunked differently (tests/compat/semantic.mjs).
/dynamic is timing-dependent even under next start (the Suspense boundary
may or may not resolve before the shell flushes), so it is gated on
status and content markers.

Measured: startup to first 200 on a static path and to the first dynamic
response, idle RSS, RSS after load, throughput, latency percentiles
(sub-millisecond, from `ab -e`), and server CPU per 1000 requests, at
concurrency 1 and 32.
"""

import argparse
import csv
import io
import json
import os
import re
import signal
import statistics
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from bench import ROOT, environment, tree_cpu_seconds, tree_rss_kib, wait_ready  # noqa: E402

HEADERS = {"x-nextrs-probe": "bench", "Cookie": "theme=dark"}
PATHS = ["/books/42", "/request-info", "/api/health", "/dynamic"]


def commands(project: Path, port: int, deno: str | None):
    next_env = {**os.environ, "NEXT_TELEMETRY_DISABLED": "1", "NODE_ENV": "production"}
    harness = str(ROOT / "experiments/route-handler-harness/harness.cjs")
    out = {
        "next-start": ([str(project / "node_modules/.bin/next"), "start", "-H", "127.0.0.1", "-p", str(port)], project, next_env),
        "nextrs-v8": (
            [str(ROOT / "target/release/nextrs"), "start", str(project), "--port", str(port), "--js-runtime", "embedded-v8"],
            ROOT,
            {**os.environ, "RUST_LOG": "error"},
        ),
        "node-harness": (["node", harness, ".", str(port)], project, next_env),
    }
    if deno:
        out["deno-harness"] = (
            [
                deno, "run",
                f"--allow-read={project},{ROOT / 'experiments/route-handler-harness'}",
                f"--allow-write={project / '.next/cache'}",
                "--allow-env", "--allow-sys", f"--allow-net=127.0.0.1:{port}",
                harness, ".", str(port),
            ],
            project,
            {**next_env, "DENO_NO_UPDATE_CHECK": "1"},
        )
    return out


class Server:
    def __init__(self, name, cmd, cwd, env, port):
        self.name, self.cmd, self.cwd, self.env, self.port = name, cmd, cwd, env, port
        self.proc = None

    def start(self):
        """Returns (seconds to first 200 on /api/health)."""
        started = time.perf_counter()
        self.proc = subprocess.Popen(
            self.cmd, cwd=self.cwd, env=self.env,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True,
        )
        wait_ready(f"http://127.0.0.1:{self.port}/api/health")
        return time.perf_counter() - started

    def stop(self):
        if self.proc and self.proc.poll() is None:
            os.killpg(self.proc.pid, signal.SIGTERM)
            try:
                self.proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(self.proc.pid, signal.SIGKILL)
                self.proc.wait()


def fetch(port, path):
    request = urllib.request.Request(f"http://127.0.0.1:{port}{path}", headers={**HEADERS, "Accept-Encoding": "identity"})
    with urllib.request.urlopen(request) as response:
        return response.status, response.read()


def run_ab(url, requests, concurrency):
    with tempfile.NamedTemporaryFile("r", suffix=".csv") as percentiles:
        cmd = ["ab", "-q", "-k", "-n", str(requests), "-c", str(concurrency), "-e", percentiles.name]
        for name, value in HEADERS.items():
            cmd += ["-H", f"{name}: {value}"]
        out = subprocess.run(cmd + [url], check=True, capture_output=True, text=True).stdout
        rows = list(csv.reader(io.StringIO(Path(percentiles.name).read_text())))[1:]
    served = {int(float(p)): float(ms) for p, ms in rows}
    result = {pct: served.get(pct) for pct in (50, 90, 95, 99)}
    rps = float(re.search(r"Requests per second:\s+([\d.]+)", out).group(1))
    failed = int(re.search(r"Failed requests:\s+(\d+)", out).group(1))
    non2xx = re.search(r"Non-2xx responses:\s+(\d+)", out)
    return {
        "requests_per_second": rps,
        "failed": failed + (int(non2xx.group(1)) if non2xx else 0),
        "p50_ms": result[50],
        "p90_ms": result[90],
        "p95_ms": result[95],
        "p99_ms": result[99],
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--project", default="fixtures/next-app")
    parser.add_argument("--out", default="benchmarks/results")
    parser.add_argument("--deno", default=None, help="Deno binary for the reference run")
    parser.add_argument("--requests", type=int, default=4000)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--startup-runs", type=int, default=5)
    parser.add_argument("--concurrency", type=int, nargs="+", default=[1, 32])
    args = parser.parse_args()

    project = (ROOT / args.project).resolve()
    ports = {"next-start": 3500, "nextrs-v8": 3501, "node-harness": 3502, "deno-harness": 3503}
    names = list(commands(project, 0, args.deno))

    def make(name):
        cmd, cwd, env = commands(project, ports[name], args.deno)[name]
        return Server(name, cmd, cwd, env, ports[name])

    results = {"environment": environment(), "config": vars(args), "startup": {}, "load": []}

    for name in names:
        ready, first_dynamic, idle = [], [], []
        for _ in range(args.startup_runs):
            server = make(name)
            try:
                started = time.perf_counter()
                ready.append(server.start())
                fetch(server.port, "/books/42")
                first_dynamic.append(time.perf_counter() - started)
                time.sleep(1.0)
                idle.append(tree_rss_kib(server.proc.pid))
            finally:
                server.stop()
        results["startup"][name] = {
            "ready_ms": statistics.median(ready) * 1000,
            "first_dynamic_response_ms": statistics.median(first_dynamic) * 1000,
            "rss_after_first_render_kib": statistics.median(idle),
        }
        print(
            f"{name:13} ready {statistics.median(ready) * 1000:7.1f} ms  first /books/42 "
            f"{statistics.median(first_dynamic) * 1000:7.1f} ms  RSS after first render {statistics.median(idle) / 1024:6.1f} MiB",
            flush=True,
        )

    servers = {name: make(name) for name in names}
    try:
        for server in servers.values():
            server.start()
        for path in PATHS:
            bodies = {name: fetch(server.port, path) for name, server in servers.items()}
            for name, (status, body) in bodies.items():
                if status != 200:
                    sys.exit(f"{name} answered {status} for {path}; refusing to benchmark")
            if path == "/dynamic":
                for name, (_, body) in bodies.items():
                    if b"Streamed after Suspense boundary" not in body or b"Noli Me Tangere" not in body:
                        sys.exit(f"{name} rendered an incomplete /dynamic; refusing to benchmark")
            else:
                reference = bodies["next-start"][1]
                for name in ("nextrs-v8", "node-harness"):
                    if bodies[name][1] != reference:
                        sys.exit(f"{name} body for {path} differs from next start; refusing to benchmark")
        print("equivalence gate passed", flush=True)

        for path in PATHS:
            for concurrency in args.concurrency:
                for name, server in servers.items():
                    url = f"http://127.0.0.1:{server.port}{path}"
                    pid = server.proc.pid
                    # /dynamic waits ~60 ms on timers per request: fewer sequential requests.
                    requests = args.requests // 10 if path == "/dynamic" and concurrency == 1 else args.requests
                    run_ab(url, max(requests // 8, 50), concurrency)
                    runs = []
                    for _ in range(args.repeats):
                        cpu_before = tree_cpu_seconds(pid)
                        measured = run_ab(url, requests, concurrency)
                        measured["requests"] = requests
                        measured["server_cpu_ms_per_request"] = (tree_cpu_seconds(pid) - cpu_before) / requests * 1000
                        measured["rss_kib_after"] = tree_rss_kib(pid)
                        runs.append(measured)
                    summary = {
                        key: statistics.median(r[key] for r in runs)
                        for key in ("requests_per_second", "p50_ms", "p90_ms", "p95_ms", "p99_ms", "server_cpu_ms_per_request")
                    }
                    summary["rss_mib_after"] = runs[-1]["rss_kib_after"] / 1024
                    summary["failed"] = sum(r["failed"] for r in runs)
                    results["load"].append({"server": name, "path": path, "concurrency": concurrency, "median": summary, "runs": runs})
                    print(
                        f"{path:14} c={concurrency:<3} {name:13} {summary['requests_per_second']:8.0f} req/s  "
                        f"p50 {summary['p50_ms']:6.2f}  p95 {summary['p95_ms']:6.2f}  p99 {summary['p99_ms']:6.2f} ms  "
                        f"cpu {summary['server_cpu_ms_per_request']:5.2f} ms/req  rss {summary['rss_mib_after']:6.1f} MiB  failed {summary['failed']}",
                        flush=True,
                    )
    finally:
        for server in servers.values():
            server.stop()

    out = ROOT / args.out
    out.mkdir(parents=True, exist_ok=True)
    path = out / f"dynamic-{time.strftime('%Y%m%dT%H%M%S')}.json"
    path.write_text(json.dumps(results, indent=2))
    print(f"wrote {path}")


if __name__ == "__main__":
    main()
