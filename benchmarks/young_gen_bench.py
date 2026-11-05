#!/usr/bin/env python3
"""V8 young-generation sizing under sustained SSR (stdlib + ab).

    python3 benchmarks/young_gen_bench.py --project fixtures/next-app \
        [--semi-space 0 1 4 16 32 64] [--isolates 1] [--seconds 60] \
        [--concurrency 32] [--out benchmarks/results]

For each --max-semi-space-size (MB; 0 = V8's default, no flag), starts
NextRS with --v8-flags, warms up, then loads /books/42 for --seconds and
records throughput, latency percentiles, server CPU per request, RSS and
the executor's V8 heap gauges (used, reserved, after full GC). A smaller
young generation lowers RSS only if it does not cost throughput; both are
reported together.
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
from dynamic_bench import Server, commands, fetch  # noqa: E402
from phase1c_bench import run_ab  # noqa: E402


def heap(port):
    with urllib.request.urlopen(f"http://127.0.0.1:{port}/metrics.json") as response:
        m = json.loads(response.read())
    mib = lambda key: sum(e[key] for e in m["executors"]) / 1048576  # noqa: E731
    return {
        "heap_used_mib": mib("heap_used_bytes"),
        "heap_total_mib": mib("heap_total_bytes"),
        "heap_after_gc_mib": mib("heap_used_after_gc_bytes"),
        "full_gcs": sum(e["full_gcs"] for e in m["executors"]),
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--project", default="fixtures/next-app")
    parser.add_argument("--semi-space", type=int, nargs="+", default=[0, 1, 4, 16, 32, 64])
    parser.add_argument("--isolates", type=int, default=1)
    parser.add_argument("--seconds", type=int, default=60)
    parser.add_argument("--concurrency", type=int, default=32)
    parser.add_argument("--repeats", type=int, default=2)
    parser.add_argument("--out", default="benchmarks/results")
    args = parser.parse_args()

    project = (ROOT / args.project).resolve()
    cmd, cwd, env = commands(project, 0, None)["nextrs-v8"]
    reference = None
    results = {"environment": environment(), "config": vars(args), "runs": []}
    for semi in args.semi_space:
        for repeat in range(args.repeats):
            port, metrics_port = 3800, 9380
            full = cmd[:]
            full[full.index("--port") + 1] = str(port)
            full += ["--isolates", str(args.isolates), "--metrics-addr", f"127.0.0.1:{metrics_port}"]
            if semi:
                full += ["--v8-flags", f"--max-semi-space-size={semi}"]
            server = Server(f"semi-{semi}", full, cwd, env, port)
            try:
                server.start()
                status, body = fetch(port, "/books/42")
                reference = reference or body
                if status != 200 or body != reference:
                    sys.exit(f"semi {semi}: /books/42 differs; refusing to measure")
                url = f"http://127.0.0.1:{port}/books/42"
                run_ab(url, 2000, args.concurrency)
                # Requests for about --seconds at the warmed-up rate.
                probe = run_ab(url, 2000, args.concurrency)
                requests = max(2000, int(probe["requests_per_second"] * args.seconds))
                pid = server.proc.pid
                cpu = tree_cpu_seconds(pid)
                measured = run_ab(url, requests, args.concurrency)
                measured["server_cpu_ms_per_request"] = (tree_cpu_seconds(pid) - cpu) / requests * 1000
                measured["rss_mib_after"] = tree_rss_kib(pid) / 1024
                measured.update(heap(metrics_port))
                measured.update({"semi_space_mb": semi, "repeat": repeat, "requests": requests})
                results["runs"].append(measured)
                print(
                    f"semi {semi or 'default':>7}  {measured['requests_per_second']:7.0f} req/s  p50 {measured['p50_ms']:6.2f}  "
                    f"p99 {measured['p99_ms']:7.2f} ms  cpu {measured['server_cpu_ms_per_request']:5.2f} ms/req  "
                    f"RSS {measured['rss_mib_after']:6.1f} MiB  heap used {measured['heap_used_mib']:5.1f}  "
                    f"reserved {measured['heap_total_mib']:6.1f}  after GC {measured['heap_after_gc_mib']:5.1f}  full GCs {measured['full_gcs']}",
                    flush=True,
                )
            finally:
                server.stop()
    out = ROOT / args.out
    out.mkdir(parents=True, exist_ok=True)
    path = out / f"young-gen-{time.strftime('%Y%m%dT%H%M%S')}.json"
    path.write_text(json.dumps(results, indent=2))
    print(f"wrote {path}")


if __name__ == "__main__":
    main()
