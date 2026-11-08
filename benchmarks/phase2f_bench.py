#!/usr/bin/env python3
"""Phase 2F benchmarks: Pages Router workloads against `next start`, and one
App Router workload against the pre-2F binary (stdlib only; load: ab).

    python3 benchmarks/phase2f_bench.py --nextrs-2e target/baseline/nextrs-2e --out benchmarks/results

Servers:

  next-start   `next start -H 0.0.0.0` (the fixture has a proxy)
  2e           the Phase 2E binary (--nextrs-2e), 1 isolate: it refuses
               Pages Router builds, so it is measured on fixtures/next-app
               only (/books/42, the Phase 1A SSR path), to show that 2F did
               not slow the App Router down
  2f           this tree, 1 isolate
  2f-i4        this tree, 4 isolates

Paths on fixtures/next-pages (built with NEXTRS_UPSTREAM pointing at the
test upstream, which runs for getServerSideProps' fetch):

  static page        /about                     (Rust in NextRS, a file in next start)
  ssr page           /books/42?ref=bench        (getServerSideProps)
  ssr data           /_next/data/<id>/books/42.json
  ssg page (hit)     /ssg/static                (getStaticProps, served from the cache)
  ssg data (hit)     /_next/data/<id>/ssg/static.json
  isr page           /isr                       (revalidate 2 s: hits, stales, background regenerations)
  api route          /api/health
  api post json      /api/echo                  (POST JSON body)
  proxy + ssr        /protected/secret          (session cookie, the proxy then getServerSideProps)

Each path's status is checked against next start before it is measured.
NextRS's /metrics.json gives executor requests per HTTP request (0 for the
Rust-served static page), queue and execution time and V8 heap.
"""

import argparse
import json
import os
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from bench import ROOT, environment  # noqa: E402
from phase1c_bench import free as port_free  # noqa: E402
import phase2b_bench  # noqa: E402

_next_port = [5800]


def free():
    """The next free port from 5800 (outside the compatibility suite's 5100-5699)."""
    while True:
        _next_port[0] += 1
        if port_free(_next_port[0]):
            return _next_port[0]


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", default="benchmarks/results")
    parser.add_argument("--nextrs-2e", required=True)
    parser.add_argument("--requests", type=int, default=4000)
    parser.add_argument("--concurrency", type=int, default=32)
    parser.add_argument("--repeats", type=int, default=3)
    args = parser.parse_args()

    nextrs = str(ROOT / "target/release/nextrs")
    upstream_port = free()
    upstream = subprocess.Popen(["node", str(ROOT / "tests/compat/upstream.mjs"), "--port", str(upstream_port)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(0.5)
    env = {**os.environ, "NEXT_TELEMETRY_DISABLED": "1", "NEXTRS_UPSTREAM": f"http://127.0.0.1:{upstream_port}"}
    body = ROOT / "target/phase2f-bench-body.json"
    body.write_text(json.dumps({"bench": True, "items": list(range(32))}))
    results = []
    try:
        pages = ROOT / "fixtures/next-pages"
        build_id = (pages / ".next/BUILD_ID").read_text().strip()
        session = "Cookie: session=synthetic-valid-session; user=u-bench"

        def servers(project):
            out = {}
            ns_port = free()
            out["next-start"] = (["npx", "next", "start", "-H", "0.0.0.0", "-p", str(ns_port)], ns_port, None)
            for name, isolates in (("2f", 1), ("2f-i4", 4)):
                port, metrics_port = free(), free()
                out[name] = ([nextrs, "start", str(project), "--port", str(port), "--js-runtime", "embedded-v8", "--isolates", str(isolates),
                              "--js-env", "NEXTRS_UPSTREAM", "--net-allow", f"http://127.0.0.1:{upstream_port}", "--net-allow-addr", "127.0.0.1",
                              "--metrics-addr", f"127.0.0.1:{metrics_port}"], port, metrics_port)
            return out

        paths = [
            ("static page", "/about", [], False),
            ("ssr page", "/books/42?ref=bench", [], False),
            ("ssr data", f"/_next/data/{build_id}/books/42.json", [], False),
            ("ssg page (hit)", "/ssg/static", [], False),
            ("ssg data (hit)", f"/_next/data/{build_id}/ssg/static.json", [], False),
            ("isr page", "/isr", [], False),
            ("api route", "/api/health", [], False),
            ("api post json", "/api/echo", [], True),
            ("proxy + ssr", "/protected/secret", [session], False),
        ]
        results += phase2b_bench.measure("next-pages", pages, servers(pages), paths, env, args, str(body), "application/json")

        # The App Router path every phase has measured, for the 2E binary.
        app = ROOT / "fixtures/next-app"
        app_servers = {}
        ns_port = free()
        app_servers["next-start"] = (["npx", "next", "start", "-H", "127.0.0.1", "-p", str(ns_port)], ns_port, None)
        for name, binary in (("2e", args.nextrs_2e), ("2f", nextrs)):
            port, metrics_port = free(), free()
            app_servers[name] = ([binary, "start", str(app), "--port", str(port), "--js-runtime", "embedded-v8", "--isolates", "1",
                                  "--metrics-addr", f"127.0.0.1:{metrics_port}"], port, metrics_port)
        results += phase2b_bench.measure("next-app", app, app_servers, [("app ssr", "/books/42", [], False)], env, args, str(body), "application/json")
    finally:
        upstream.terminate()

    out = Path(args.out)
    out.mkdir(parents=True, exist_ok=True)
    stamp = time.strftime("%Y%m%dT%H%M%S")
    path = out / f"phase2f-{stamp}.json"
    path.write_text(json.dumps({"environment": environment(), "requests": args.requests, "concurrency": args.concurrency,
                                "repeats": args.repeats, "load": results}, indent=2))
    print(f"wrote {path}")


if __name__ == "__main__":
    main()
