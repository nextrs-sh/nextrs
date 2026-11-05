#!/usr/bin/env python3
"""Phase 2A benchmarks: the proxy (proxy.ts) and next.config routing on
fixtures/next-proxy, against `next start` (stdlib only; load: ab).

    python3 benchmarks/phase2a_bench.py --project fixtures/next-proxy --out benchmarks/results

Servers (one build, one upstream, tests/compat/upstream.mjs):

  next-start   `next start -H 0.0.0.0` (on 127.0.0.1 it proxies its own
               proxy rewrites over HTTP; see tests/compat/proxy.mjs)
  2a           this tree, 1 isolate
  2a-i4        4 isolates

Paths:

  not matched     /excluded              prerendered, excluded by the matcher
                  /api/health            route handler, excluded
                  /_next/static/<chunk>  static asset, excluded
  matched         /target                prerendered, proxy continues
                  /asset.txt             public file, proxy continues
                  /api/echo              route handler, proxy continues
                  /isr                   ISR entry within its hour
                  /dynamic               SSR, proxy sets request headers
  answered        /proxy/redirect        the proxy redirects (307)
  by the proxy    /proxy/json            the proxy answers directly
  rewritten       /proxy/rewrite         proxy rewrite to a prerendered page
                  /af/hello              next.config afterFiles rewrite
                  /old-blog/x            next.config redirect (before the proxy)

Each path's status is checked against next start before it is measured.
NextRS's /metrics.json gives executor requests per HTTP request (1 for a
proxied page served from Rust, 2 for a proxied SSR), queue time and
execution time. Results go to <out>/phase2a-<time>.json.
"""

import argparse
import json
import statistics
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from bench import ROOT, environment, tree_cpu_seconds, tree_rss_kib, wait_ready  # noqa: E402
from dynamic_bench import Server  # noqa: E402
from phase1c_bench import free, metrics  # noqa: E402
from phase1e_bench import ab  # noqa: E402

UPSTREAM_PORT = 4985


def status(port, path):
    request = urllib.request.Request(f"http://127.0.0.1:{port}{path}", headers={"Accept-Encoding": "identity"})

    class NoRedirect(urllib.request.HTTPRedirectHandler):
        def redirect_request(self, *args, **kwargs):
            return None

    opener = urllib.request.build_opener(NoRedirect)
    try:
        with opener.open(request) as response:
            return response.status
    except urllib.error.HTTPError as error:
        return error.code


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--project", default="fixtures/next-proxy")
    parser.add_argument("--out", default="benchmarks/results")
    parser.add_argument("--concurrency", type=int, nargs="+", default=[32])
    parser.add_argument("--requests", type=int, default=4000)
    parser.add_argument("--repeats", type=int, default=2)
    parser.add_argument("--only", nargs="*", default=None)
    args = parser.parse_args()

    project = (ROOT / args.project).resolve()
    chunk = next((project / ".next/static/chunks").glob("*.js")).name
    paths = [
        ("not-matched prerendered", "/excluded"),
        ("not-matched route", "/api/health"),
        ("not-matched static", f"/_next/static/chunks/{chunk}"),
        ("proxy prerendered", "/target"),
        ("proxy public file", "/asset.txt"),
        ("proxy route handler", "/api/echo"),
        ("proxy ISR hit", "/isr"),
        ("proxy SSR", "/dynamic"),
        ("proxy redirect", "/proxy/redirect"),
        ("proxy direct JSON", "/proxy/json"),
        ("proxy rewrite", "/proxy/rewrite"),
        ("config rewrite", "/af/hello"),
        ("config redirect", "/old-blog/x"),
    ]

    upstream = f"http://127.0.0.1:{UPSTREAM_PORT}"
    if not free(UPSTREAM_PORT):
        sys.exit(f"port {UPSTREAM_PORT} is in use")
    upstream_proc = subprocess.Popen(
        ["node", str(ROOT / "tests/compat/upstream.mjs"), "--port", str(UPSTREAM_PORT)],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True,
    )
    env = {"PATH": __import__("os").environ["PATH"], "HOME": __import__("os").environ["HOME"], "NEXT_TELEMETRY_DISABLED": "1", "NEXTRS_UPSTREAM": upstream}
    nextrs = str(ROOT / "target/release/nextrs")
    net = ["--js-env", "NEXTRS_UPSTREAM", "--net-allow", upstream, "--net-allow-addr", "127.0.0.1"]
    servers = {
        "next-start": (["npx", "next", "start", "-H", "0.0.0.0", "-p", "9420"], 9420, None),
        "2a": ([nextrs, "start", str(project), "--port", "9421", "--js-runtime", "embedded-v8", *net, "--metrics-addr", "127.0.0.1:9431"], 9421, 9431),
        "2a-i4": ([nextrs, "start", str(project), "--port", "9422", "--js-runtime", "embedded-v8", "--isolates", "4", *net, "--metrics-addr", "127.0.0.1:9432"], 9422, 9432),
    }
    if args.only:
        servers = {k: v for k, v in servers.items() if k in args.only}

    results = {"environment": environment(), "config": vars(args), "load": []}
    expected = {}
    try:
        for name, (cmd, port, metrics_port) in servers.items():
            server = Server(name, cmd, str(project), env, port)
            server.start()
            try:
                for label, path in paths:
                    got = status(port, path)
                    expected.setdefault(path, got)
                    if got != expected[path]:
                        print(f"{name} {path}: status {got}, next start {expected[path]}; skipped", flush=True)
                        continue
                    for concurrency in args.concurrency:
                        url = f"http://127.0.0.1:{port}{path}"
                        ab(url, max(args.requests // 8, 2 * concurrency), concurrency)  # warm-up
                        before = metrics(metrics_port)
                        runs = []
                        for _ in range(args.repeats):
                            cpu = tree_cpu_seconds(server.proc.pid)
                            run = ab(url, args.requests, concurrency)
                            run["server_cpu_ms_per_request"] = (tree_cpu_seconds(server.proc.pid) - cpu) / args.requests * 1000
                            runs.append(run)
                        summary = {k: statistics.median(r[k] for r in runs) for k in ("requests_per_second", "p50_ms", "p95_ms", "p99_ms", "server_cpu_ms_per_request")}
                        summary["failed"] = sum(r["failed"] for r in runs)
                        summary["rss_mib_after"] = tree_rss_kib(server.proc.pid) / 1024
                        after = metrics(metrics_port)
                        if before and after:
                            http = args.requests * args.repeats
                            calls = after["requests_completed"] - before["requests_completed"]
                            summary["executor_requests_per_http_request"] = calls / http
                            waits = after["queue_waits"] - before["queue_waits"]
                            summary["queue_ms_mean"] = (after["queue_wait_seconds_total"] - before["queue_wait_seconds_total"]) / max(waits, 1) * 1000 if waits else 0.0
                            summary["execution_ms_mean"] = (after["execution_seconds_total"] - before["execution_seconds_total"]) / max(calls, 1) * 1000
                            summary["v8_heap_used_mib"] = sum(e["heap_used_bytes"] for e in after["executors"]) / 2**20
                        results["load"].append({"server": name, "label": label, "path": path, "concurrency": concurrency, "median": summary, "runs": runs})
                        extra = ""
                        if "executor_requests_per_http_request" in summary:
                            extra = f"  js/req {summary['executor_requests_per_http_request']:.2f}  queue {summary['queue_ms_mean']:.2f} ms  exec {summary['execution_ms_mean']:.2f} ms  heap {summary['v8_heap_used_mib']:.0f} MiB"
                        print(f"{label:24} c={concurrency:<3} {name:10} {summary['requests_per_second']:8.0f} req/s  p50 {summary['p50_ms']:7.2f}  p95 {summary['p95_ms']:7.2f}  p99 {summary['p99_ms']:7.2f} ms  cpu {summary['server_cpu_ms_per_request']:5.2f} ms/req  rss {summary['rss_mib_after']:6.1f} MiB  failed {summary['failed']}{extra}", flush=True)
            finally:
                server.stop()
                time.sleep(1)
    finally:
        __import__("os").killpg(upstream_proc.pid, 15)
    out = ROOT / args.out
    out.mkdir(parents=True, exist_ok=True)
    target = out / f"phase2a-{time.strftime('%Y%m%dT%H%M%S')}.json"
    target.write_text(json.dumps(results, indent=2))
    print(f"wrote {target}")


if __name__ == "__main__":
    main()
