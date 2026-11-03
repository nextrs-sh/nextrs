#!/usr/bin/env python3
"""Phase 0 baseline benchmarks (stdlib only; load generator: ApacheBench).

    python3 benchmarks/bench.py --project fixtures/next-app --out benchmarks/results

Servers (same machine, same .next build, same request, no Accept-Encoding so
no server compresses, responses verified byte-identical before measuring):

  next-start     `next start` (Node.js)
  nextrs         `nextrs start` release build (Rust, static only)
  node-harness   experiments/route-handler-harness (Node.js, calls route
                 handlers directly; used only for dynamic comparisons with
                 next-start since nextrs cannot render dynamic routes)

Measured: startup (spawn -> first 200 on /), idle RSS of the process tree,
throughput + latency percentiles (ab -k), and server CPU seconds consumed per
1000 requests.
"""

import argparse
import json
import os
import platform
import re
import shutil
import signal
import statistics
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CLK_TCK = os.sysconf("SC_CLK_TCK")
PAGE_KIB = os.sysconf("SC_PAGE_SIZE") // 1024


def server_commands(project: Path, port: int):
    next_bin = project / "node_modules/.bin/next"
    env = {**os.environ, "NEXT_TELEMETRY_DISABLED": "1", "NODE_ENV": "production"}
    return {
        "next-start": ([str(next_bin), "start", "-H", "127.0.0.1", "-p", str(port)], project, env),
        "nextrs": (
            [str(ROOT / "target/release/nextrs"), "start", str(project), "--port", str(port)],
            ROOT,
            {**os.environ, "RUST_LOG": "warn"},
        ),
        "node-harness": (
            ["node", str(ROOT / "experiments/route-handler-harness/harness.cjs"), ".", str(port)],
            project,
            env,
        ),
    }


def tree_pids(root_pid: int):
    children = {}
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            fields = (entry / "stat").read_text().rsplit(")", 1)[1].split()
        except OSError:
            continue
        children.setdefault(int(fields[1]), []).append(int(entry.name))
    pids, stack = [], [root_pid]
    while stack:
        pid = stack.pop()
        pids.append(pid)
        stack.extend(children.get(pid, []))
    return pids


def tree_rss_kib(root_pid: int) -> int:
    total = 0
    for pid in tree_pids(root_pid):
        try:
            total += int(Path(f"/proc/{pid}/statm").read_text().split()[1]) * PAGE_KIB
        except OSError:
            pass
    return total


def tree_cpu_seconds(root_pid: int) -> float:
    ticks = 0
    for pid in tree_pids(root_pid):
        try:
            fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
            ticks += int(fields[11]) + int(fields[12])  # utime + stime
        except OSError:
            pass
    return ticks / CLK_TCK


def wait_ready(url: str, timeout: float = 60.0) -> float:
    start = time.perf_counter()
    while time.perf_counter() - start < timeout:
        try:
            with urllib.request.urlopen(url, timeout=1) as response:
                if response.status == 200:
                    response.read()
                    return time.perf_counter() - start
        except OSError:
            time.sleep(0.005)
    raise RuntimeError(f"{url} not ready after {timeout}s")


class Server:
    def __init__(self, name, project, port):
        self.name, self.port = name, port
        self.cmd, self.cwd, self.env = server_commands(project, port)[name]
        self.proc = None

    def start(self) -> float:
        started = time.perf_counter()
        self.proc = subprocess.Popen(
            self.cmd, cwd=self.cwd, env=self.env,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True,
        )
        wait_ready(f"http://127.0.0.1:{self.port}/")
        return time.perf_counter() - started

    def stop(self):
        if self.proc and self.proc.poll() is None:
            os.killpg(self.proc.pid, signal.SIGTERM)
            try:
                self.proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(self.proc.pid, signal.SIGKILL)
                self.proc.wait()


AB_PATTERNS = {
    "requests_per_second": r"Requests per second:\s+([\d.]+)",
    "failed": r"Failed requests:\s+(\d+)",
    "non_2xx": r"Non-2xx responses:\s+(\d+)",
    "mean_ms": r"Time per request:\s+([\d.]+) \[ms\] \(mean\)",
    "transfer_kib_s": r"Transfer rate:\s+([\d.]+)",
}


def run_ab(url: str, requests: int, concurrency: int):
    out = subprocess.run(
        ["ab", "-q", "-k", "-n", str(requests), "-c", str(concurrency), url],
        check=True, capture_output=True, text=True,
    ).stdout
    result = {}
    for key, pattern in AB_PATTERNS.items():
        match = re.search(pattern, out)
        result[key] = float(match.group(1)) if match else 0.0
    for pct in (50, 90, 99, 100):
        match = re.search(rf"^\s+{pct}%\s+(\d+)", out, re.M)
        result[f"p{pct}_ms"] = int(match.group(1)) if match else None
    return result


def fetch(url):
    with urllib.request.urlopen(urllib.request.Request(url, headers={"Accept-Encoding": "identity"})) as r:
        return r.status, r.read()


def environment():
    cpu = next(
        (line.split(":", 1)[1].strip() for line in Path("/proc/cpuinfo").read_text().splitlines()
         if line.startswith("model name")),
        platform.processor(),
    )
    version = lambda cmd: subprocess.run(cmd, capture_output=True, text=True).stdout.strip()
    return {
        "cpu": cpu,
        "logical_cpus": os.cpu_count(),
        "kernel": platform.release(),
        "node": version(["node", "--version"]),
        "rustc": version(["rustc", "--version"]),
        "ab": version(["ab", "-V"]).splitlines()[0],
        "mem_total_kib": int(re.search(r"MemTotal:\s+(\d+)", Path("/proc/meminfo").read_text()).group(1)),
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--project", default="fixtures/next-app")
    parser.add_argument("--out", default="benchmarks/results")
    parser.add_argument("--requests", type=int, default=20000)
    parser.add_argument("--concurrency", type=int, default=32)
    parser.add_argument("--warmup", type=int, default=2000)
    parser.add_argument("--repeats", type=int, default=3)
    parser.add_argument("--startup-runs", type=int, default=5)
    args = parser.parse_args()

    project = (ROOT / args.project).resolve()
    if shutil.which("ab") is None:
        sys.exit("ApacheBench (ab) is required")
    build_manifest = json.loads((project / ".next/build-manifest.json").read_text())
    chunk = "/_next/" + build_manifest["rootMainFiles"][0]

    # (server, path) pairs. Equivalence is checked before any measurement.
    static_cases = [("next-start", "/"), ("nextrs", "/"), ("next-start", chunk), ("nextrs", chunk)]
    dynamic_cases = [("next-start", "/books/42"), ("node-harness", "/books/42")]
    ports = {"next-start": 3400, "nextrs": 3401, "node-harness": 3402}

    results = {"environment": environment(), "config": vars(args), "startup": {}, "idle_rss_kib": {}, "load": []}

    # Startup and idle memory.
    for name in ports:
        times, rss = [], []
        for _ in range(args.startup_runs):
            server = Server(name, project, ports[name])
            try:
                times.append(server.start())
                time.sleep(1.0)
                rss.append(tree_rss_kib(server.proc.pid))
            finally:
                server.stop()
        results["startup"][name] = {"median_ms": statistics.median(times) * 1000, "runs_ms": [t * 1000 for t in times]}
        results["idle_rss_kib"][name] = {"median": statistics.median(rss), "runs": rss}
        print(f"{name:13} startup median {statistics.median(times) * 1000:7.1f} ms   idle RSS {statistics.median(rss) / 1024:6.1f} MiB", flush=True)

    servers = {name: Server(name, project, port) for name, port in ports.items()}
    try:
        for server in servers.values():
            server.start()
        # Equivalence gate: identical status and body bytes across servers.
        for group in (static_cases, dynamic_cases):
            by_path = {}
            for name, path in group:
                by_path.setdefault(path, []).append((name, fetch(f"http://127.0.0.1:{ports[name]}{path}")))
            for path, responses in by_path.items():
                (_, first), *rest = responses
                for name, response in rest:
                    if response != first:
                        sys.exit(f"responses for {path} differ between servers ({name}); refusing to benchmark")
        print("equivalence check passed: identical status and body for every benchmarked path", flush=True)

        for name, path in static_cases + dynamic_cases:
            url = f"http://127.0.0.1:{ports[name]}{path}"
            pid = servers[name].proc.pid
            run_ab(url, args.warmup, args.concurrency)
            runs = []
            for _ in range(args.repeats):
                cpu_before = tree_cpu_seconds(pid)
                measured = run_ab(url, args.requests, args.concurrency)
                measured["server_cpu_s_per_1k"] = (tree_cpu_seconds(pid) - cpu_before) / args.requests * 1000
                measured["rss_kib_after"] = tree_rss_kib(pid)
                runs.append(measured)
            best = max(runs, key=lambda r: r["requests_per_second"])
            median_rps = statistics.median(r["requests_per_second"] for r in runs)
            results["load"].append({"server": name, "path": path, "median_rps": median_rps, "runs": runs})
            print(
                f"{name:13} {path:48} median {median_rps:9.0f} req/s  p50 {best['p50_ms']} ms  p99 {best['p99_ms']} ms"
                f"  cpu {statistics.median(r['server_cpu_s_per_1k'] for r in runs) * 1000:6.1f} ms/1k req"
                f"  failed {int(sum(r['failed'] + r['non_2xx'] for r in runs))}",
                flush=True,
            )
    finally:
        for server in servers.values():
            server.stop()

    out = ROOT / args.out
    out.mkdir(parents=True, exist_ok=True)
    stamp = time.strftime("%Y%m%dT%H%M%S")
    (out / f"baseline-{stamp}.json").write_text(json.dumps(results, indent=2))
    print(f"wrote {out / f'baseline-{stamp}.json'}")


if __name__ == "__main__":
    main()
