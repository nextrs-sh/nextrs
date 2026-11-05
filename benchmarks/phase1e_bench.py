#!/usr/bin/env python3
"""Phase 1E benchmarks: ISR, regeneration, revalidation from Server Actions
and "use cache", against `next start` and the Phase 1D binary (stdlib only;
load: ab).

    python3 benchmarks/phase1e_bench.py --project fixtures/next-app \
        --cc-project fixtures/next-cache-components \
        --nextrs-1d /path/to/phase-1d/nextrs --out benchmarks/results

Servers (one .next build each, one upstream, tests/compat/upstream.mjs):

  next-start   `next start`
  1d           the Phase 1D binary: no ISR (its ISR routes answer 501, so
               only the prerendered and SSR paths are measured)
  1e           this tree, shared cache in memory, 1 isolate
  1e-disk      the same with --cache-dir
  1e-i4        4 isolates

Paths, kept apart because they measure different things:

  cache hits      /actions/static-time   prerendered, never revalidates
                                         (NextRS: answered from Rust memory)
                  /isr/two               ISR entry within its hour (Next's
                                         handler serves it from the cache)
                  /cached?shelf=bench    a "use cache" hit inside a dynamic
                                         render (Cache Components fixture)
  regeneration    /isr/timed             revalidate 2 s under constant load:
                                         stale responses plus background
                                         regenerations (counted upstream)
  dynamic         /books/42              SSR, no cache: regression check
  revalidation    POST /isr/tagged       a Server Action that calls
                                         revalidatePath / revalidateTag
                                         (each re-renders or marks stale)

Before a server is measured, each path's response is checked: the same
status and generation semantics as next start (an ISR HIT, the static
page's stamp, the SSR body byte-identical). Results go to
<out>/phase1e-<time>.json.
"""

import argparse
import json
import os
import re
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from bench import ROOT, environment, tree_cpu_seconds, tree_rss_kib  # noqa: E402
from dynamic_bench import Server, commands  # noqa: E402
from phase1c_bench import free, metrics  # noqa: E402

UPSTREAM_PORT = 4975
BOUNDARY = "nextrsbench"


def get(port, path, headers=None):
    request = urllib.request.Request(f"http://127.0.0.1:{port}{path}", headers={"Accept-Encoding": "identity", **(headers or {})})
    try:
        with urllib.request.urlopen(request) as response:
            return response.status, dict(response.headers), response.read()
    except urllib.error.HTTPError as error:
        return error.code, dict(error.headers), error.read()


def upstream_hits(key):
    with urllib.request.urlopen(f"http://127.0.0.1:{UPSTREAM_PORT}/hits?key={key}") as response:
        return json.loads(response.read())["hits"]


def ab(url, requests, concurrency, post=None, headers=None):
    import csv
    import io

    with tempfile.NamedTemporaryFile("r", suffix=".csv") as percentiles:
        cmd = ["ab", "-q", "-k", "-n", str(requests), "-c", str(concurrency), "-e", percentiles.name]
        for name, value in (headers or {}).items():
            cmd += ["-H", f"{name}: {value}"]
        if post:
            cmd += ["-p", post, "-T", f"multipart/form-data; boundary={BOUNDARY}"]
        out = subprocess.run(cmd + [url], check=True, capture_output=True, text=True).stdout
        rows = list(csv.reader(io.StringIO(Path(percentiles.name).read_text())))[1:]
    served = {int(float(p)): float(ms) for p, ms in rows}
    breakdown = re.search(r"Connect: (\d+), Receive: (\d+), Length: \d+, Exceptions: (\d+)", out)
    non2xx = re.search(r"Non-2xx responses:\s+(\d+)", out)
    return {
        "requests_per_second": float(re.search(r"Requests per second:\s+([\d.]+)", out).group(1)),
        # ab's "Length" failures are pages whose stamp changed; not errors.
        "failed": (sum(int(n) for n in breakdown.groups()) if breakdown else 0) + (int(non2xx.group(1)) if non2xx else 0),
        "p50_ms": served.get(50),
        "p95_ms": served.get(95),
        "p99_ms": served.get(99),
    }


def action_body(how, directory):
    parts = [("_1_how", how), ("_1_id", "bench"), ("0", '["$K1"]')]
    body = "".join(f"--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{n}\"\r\n\r\n{v}\r\n" for n, v in parts) + f"--{BOUNDARY}--\r\n"
    path = Path(directory) / f"action-{how}.txt"
    path.write_bytes(body.encode())
    return str(path)


def action_id(project, name, filename):
    manifest = json.loads((project / ".next/server/server-reference-manifest.json").read_text())
    return next(k for k, v in manifest["node"].items() if v["exportedName"] == name and v["filename"] == filename)


def stamp(body):
    m = re.search(rb"stamp-(?:<!-- -->)?(\d+)", body)
    return m.group(1) if m else None


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--project", default="fixtures/next-app")
    parser.add_argument("--cc-project", default="fixtures/next-cache-components")
    parser.add_argument("--nextrs-1d", default=None)
    parser.add_argument("--out", default="benchmarks/results")
    parser.add_argument("--concurrency", type=int, nargs="+", default=[32])
    parser.add_argument("--requests", type=int, default=4000)
    parser.add_argument("--repeats", type=int, default=2)
    parser.add_argument("--only", nargs="*", default=None)
    args = parser.parse_args()

    upstream = f"http://127.0.0.1:{UPSTREAM_PORT}"
    work = Path(tempfile.mkdtemp(prefix="nextrs-bench-1e-"))
    results = {"environment": environment(), "config": {**vars(args), "nextrs_1d": Path(args.nextrs_1d).name if args.nextrs_1d else None}, "load": []}
    if not free(UPSTREAM_PORT):
        sys.exit(f"port {UPSTREAM_PORT} is in use")
    upstream_proc = subprocess.Popen(
        ["node", str(ROOT / "tests/compat/upstream.mjs"), "--port", str(UPSTREAM_PORT)],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True,
    )
    time.sleep(0.5)

    def variants(project):
        base = commands(project, 0, None)
        next_cmd, next_cwd, next_env = base["next-start"]
        rs_cmd, rs_cwd, rs_env = base["nextrs-v8"]
        env = {"NEXTRS_UPSTREAM": upstream}
        net = ["--js-env", "NEXTRS_UPSTREAM", "--net-allow", upstream, "--net-allow-addr", "127.0.0.1"]
        out = {"next-start": ((next_cmd, next_cwd, {**next_env, **env}), None)}
        if args.nextrs_1d:
            # 1D refuses ISR builds at startup; its ISR routes then answer 501.
            out["1d"] = (([args.nextrs_1d] + rs_cmd[1:] + net + ["--allow-unsupported"], rs_cwd, {**rs_env, **env}), 9411)
        out["1e"] = ((rs_cmd + net, rs_cwd, {**rs_env, **env}), 9412)
        out["1e-disk"] = ((rs_cmd + net + ["--cache-dir", str(work / "cache")], rs_cwd, {**rs_env, **env}), 9413)
        out["1e-i4"] = ((rs_cmd + net + ["--isolates", "4"], rs_cwd, {**rs_env, **env}), 9414)
        if args.only:
            out = {k: v for k, v in out.items() if k in args.only}
        return out

    def make(spec, name, port):
        (cmd, cwd, env), metrics_port = spec
        cmd = cmd[:]
        for flag in ("-p", "--port"):
            if flag in cmd:
                cmd[cmd.index(flag) + 1] = str(port)
        if metrics_port:
            cmd += ["--metrics-addr", f"127.0.0.1:{metrics_port}"]
        return Server(name, cmd, cwd, env, port)

    def measure(server, metrics_port, label, path, concurrency, post=None, headers=None, upstream_key=None):
        url = f"http://127.0.0.1:{server.port}{path}"
        ab(url, max(args.requests // 8, 2 * concurrency), concurrency, post, headers)  # warm-up
        before = metrics(metrics_port)
        renders_before = upstream_hits(upstream_key) if upstream_key else None
        runs = []
        for _ in range(args.repeats):
            cpu = tree_cpu_seconds(server.proc.pid)
            run = ab(url, args.requests, concurrency, post, headers)
            run["server_cpu_ms_per_request"] = (tree_cpu_seconds(server.proc.pid) - cpu) / args.requests * 1000
            runs.append(run)
        summary = {k: statistics.median(r[k] for r in runs) for k in ("requests_per_second", "p50_ms", "p95_ms", "p99_ms", "server_cpu_ms_per_request")}
        summary["failed"] = sum(r["failed"] for r in runs)
        summary["rss_mib_after"] = tree_rss_kib(server.proc.pid) / 1024
        if upstream_key:
            summary["renders"] = upstream_hits(upstream_key) - renders_before
        after = metrics(metrics_port)
        if before and after:
            summary["v8_heap_used_mib"] = sum(e["heap_used_bytes"] for e in after["executors"]) / 2**20
            if after.get("cache") and before.get("cache"):
                c0, c1 = before["cache"], after["cache"]
                summary["cache_response_sets"] = c1.get("response_sets", 0) - c0.get("response_sets", 0)
                summary["cache_regeneration_waits"] = c1.get("regeneration_waits", 0) - c0.get("regeneration_waits", 0)
                summary["cache_storage_mib"] = c1["storage_bytes"] / 2**20
                summary["cache_memory_mib"] = c1["memory_bytes"] / 2**20
        results["load"].append({"server": server.name, "label": label, "path": path, "concurrency": concurrency, "median": summary, "runs": runs})
        extra = "".join(f"  {k} {summary[k]}" for k in ("renders", "cache_response_sets", "cache_regeneration_waits") if k in summary)
        print(f"{label:22} c={concurrency:<3} {server.name:10} {summary['requests_per_second']:8.0f} req/s  p50 {summary['p50_ms']:7.2f}  p95 {summary['p95_ms']:7.2f}  p99 {summary['p99_ms']:7.2f} ms  cpu {summary['server_cpu_ms_per_request']:5.2f} ms/req  rss {summary['rss_mib_after']:6.1f} MiB  failed {summary['failed']}{extra}", flush=True)

    try:
        # --- The main fixture ------------------------------------------------
        project = (ROOT / args.project).resolve()
        # next start writes regenerated pages and fetch results into the
        # build (server/route-cache, cache/fetch-cache); earlier runs' files
        # would make it serve those instead of the build's prerenders.
        for written in (project / ".next/server/route-cache", project / ".next/cache/fetch-cache"):
            shutil.rmtree(written, ignore_errors=True)
        build_stamp = stamp((project / ".next/server/app/actions/static-time.html").read_bytes())
        tagged_id = action_id(project, "isrInvalidate", "app/isr/actions.ts")
        reference = {}
        for i, (name, spec) in enumerate(variants(project).items()):
            shutil.rmtree(work / "cache", ignore_errors=True)
            server = make(spec, name, 3900 + i)
            metrics_port = spec[1]
            try:
                server.start()
                # Correctness before speed.
                status, _, body = get(server.port, "/books/42")
                reference.setdefault("ssr", body)
                if status != 200 or body != reference["ssr"]:
                    sys.exit(f"{name}: /books/42 differs from next start")
                status, _, body = get(server.port, "/actions/static-time")
                if status != 200 or stamp(body) != build_stamp:
                    sys.exit(f"{name}: /actions/static-time is not the prerender")
                isr = name != "1d"
                if isr:
                    get(server.port, "/isr/two")
                    status, headers, body = get(server.port, "/isr/two")
                    if status != 200 or headers.get("x-nextjs-cache") != "HIT" or b"slug two" not in body:
                        sys.exit(f"{name}: /isr/two is not an ISR cache hit ({status} {headers.get('x-nextjs-cache')})")
                for c in args.concurrency:
                    measure(server, metrics_port, "prerendered (static)", "/actions/static-time", c)
                    if isr:
                        measure(server, metrics_port, "isr hit", "/isr/two", c)
                        measure(server, metrics_port, "isr stale+regenerate", "/isr/timed", c, upstream_key="isr-timed")
                    measure(server, metrics_port, "ssr (no cache)", "/books/42", c)
                    if isr:
                        hdrs = {"next-action": tagged_id, "origin": f"http://127.0.0.1:{server.port}", "accept": "text/x-component"}
                        for how in ("revalidatePath", "revalidateTag"):
                            measure(server, metrics_port, f"action {how}", "/isr/tagged", c, post=action_body(how, work), headers=hdrs, upstream_key="isr-tagged")
            finally:
                server.stop()

        # --- Cache Components ------------------------------------------------
        cc = (ROOT / args.cc_project).resolve()
        if (cc / ".next/BUILD_ID").exists():
            for i, (name, spec) in enumerate(variants(cc).items()):
                if name == "1d":
                    continue  # serves the PPR shell without resuming it
                shutil.rmtree(work / "cache", ignore_errors=True)
                server = make(spec, name, 3950 + i)
                try:
                    server.start()
                    _, _, a = get(server.port, "/cached?shelf=bench")
                    _, _, b = get(server.port, "/cached?shelf=bench")
                    at = lambda body: re.findall(rb"at=(\d+)", body)[-1:]  # noqa: E731
                    if not at(a) or at(a) != at(b):
                        sys.exit(f"{name}: the use-cache value is not cached")
                    for c in args.concurrency:
                        measure(server, spec[1], "use cache hit", "/cached?shelf=bench", c, upstream_key="cc-books-bench")
                finally:
                    server.stop()
    finally:
        os.killpg(upstream_proc.pid, 15)
        shutil.rmtree(work, ignore_errors=True)

    out = ROOT / args.out
    out.mkdir(parents=True, exist_ok=True)
    path = out / f"phase1e-{time.strftime('%Y%m%dT%H%M%S')}.json"
    path.write_text(json.dumps(results, indent=2))
    print(f"wrote {path}")


if __name__ == "__main__":
    main()
