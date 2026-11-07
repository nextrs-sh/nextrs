#!/usr/bin/env python3
"""Phase 2B benchmarks: real-world proxies against `next start` and the
Phase 2A binary (stdlib only; load: ab).

    python3 benchmarks/phase2b_bench.py --nextrs-2a target/baseline/nextrs-2a --out benchmarks/results

Servers per fixture (each with the test upstream, tests/compat/upstream.mjs):

  next-start   `next start -H 0.0.0.0`
  2a           the Phase 2A binary (--nextrs-2a), 1 isolate: only on paths
               whose behaviour it shares (it cannot verify HS256 sessions
               or forward external rewrites)
  2b           this tree, 1 isolate
  2b-i4        this tree, 4 isolates

Paths:

  fixtures/next-proxy           unmatched prerendered, matched prerendered,
                                proxy redirect, proxy rewrite, proxy + ISR,
                                proxy + Server Action (the side-effect-free
                                `inspect` action)
  fixtures/next-auth-proxy      authenticated proxy request (a valid HS256
                                session, jose verifies it in the proxy) on a
                                prerendered page, invalid-session redirect
  fixtures/next-platform-proxy  proxy with an outbound fetch, external
                                rewrite (config)

Each path's status is checked against next start before it is measured,
and paths are only compared with equal status. The fixtures must be built
(the platform one against --upstream-port, as this script does not
rebuild them). NextRS's /metrics.json gives executor requests per HTTP
request, queue and execution time, and outbound fetch latency.
"""

import argparse
import base64
import hashlib
import hmac
import json
import os
import statistics
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from bench import ROOT, environment, tree_cpu_seconds, tree_rss_kib  # noqa: E402
from dynamic_bench import Server  # noqa: E402
from phase1c_bench import free, metrics  # noqa: E402
from phase2a_bench import status  # noqa: E402


def ab(url, n, c, headers=(), body=None, content_type=None):
    """`ab -k` with extra headers and an optional POST body file."""
    cmd = ["ab", "-q", "-k", "-n", str(n), "-c", str(c), "-s", "60"]
    for h in headers:
        cmd += ["-H", h]
    if body is not None:
        cmd += ["-p", body, "-T", content_type]
    cmd.append(url)
    out = subprocess.run(cmd, capture_output=True, text=True, check=False).stdout
    def field(name):
        for line in out.splitlines():
            if line.startswith(name):
                return line.split(":", 1)[1].split()[0]
        return None
    pct = {}
    for line in out.splitlines():
        parts = line.split()
        if len(parts) == 2 and parts[0].endswith("%"):
            pct[parts[0]] = float(parts[1])
    return {
        "requests_per_second": float(field("Requests per second") or 0),
        "failed": int(field("Failed requests") or 0),
        "non_2xx": int(field("Non-2xx responses") or 0),
        "p50_ms": pct.get("50%", 0.0),
        "p95_ms": pct.get("95%", 0.0),
        "p99_ms": pct.get("99%", 0.0),
    }


def session_token(secret, user, name):
    """An HS256 JWT as the fixture's jose code issues it."""
    b64 = lambda data: base64.urlsafe_b64encode(data).rstrip(b"=").decode()
    now = int(time.time())
    header = b64(json.dumps({"alg": "HS256"}, separators=(",", ":")).encode())
    payload = b64(json.dumps({"userId": user, "name": name, "iat": now, "exp": now + 7200}, separators=(",", ":")).encode())
    signature = b64(hmac.new(secret.encode(), f"{header}.{payload}".encode(), hashlib.sha256).digest())
    return f"{header}.{payload}.{signature}"


def action_body(path, action_field_prefix="_1_"):
    boundary = "----nextrsbench"
    parts = [f'--{boundary}\r\nContent-Disposition: form-data; name="{action_field_prefix}value"\r\n\r\nbench\r\n',
             f'--{boundary}\r\nContent-Disposition: form-data; name="0"\r\n\r\n["$K1"]\r\n', f"--{boundary}--\r\n"]
    Path(path).write_text("".join(parts))
    return f"multipart/form-data; boundary={boundary}"



def measure(fixture, project, servers, paths, env, args, body_file, content_type):
    """Measures `paths` ((label, path, headers, post[, include the baseline
    binary])) on each server ({name: (cmd, port, metrics port)}); a path is
    measured only where its status equals next start's."""
    load = []
    expected = {}
    for name, (cmd, port, metrics_port) in servers.items():
        server = Server(name, cmd, str(project), env, port)
        server.start()
        try:
            for label, path, headers, post, *rest in paths:
                with_baseline = rest[0] if rest else True
                if name in ("2a", "2b") and not with_baseline:
                    continue
                headers = [h.replace("ORIGIN", f"http://127.0.0.1:{port}") for h in headers]
                url = f"http://127.0.0.1:{port}{path}"
                req = urllib.request.Request(url, data=Path(body_file).read_bytes() if post else None,
                                             headers={**{h.split(": ", 1)[0]: h.split(": ", 1)[1] for h in headers}, "Accept-Encoding": "identity",
                                                      **({"Content-Type": content_type} if post else {})})
                try:
                    got = urllib.request.build_opener(type("N", (urllib.request.HTTPRedirectHandler,), {"redirect_request": lambda *a, **k: None})).open(req).status
                except urllib.error.HTTPError as error:
                    got = error.code
                key = (label, path)
                expected.setdefault(key, got)
                if got != expected[key]:
                    print(f"{fixture} {name} {label}: status {got}, next start {expected[key]}; skipped", flush=True)
                    continue
                post_args = (body_file, content_type) if post else (None, None)
                ab(url, max(args.requests // 8, 2 * args.concurrency), args.concurrency, headers, *post_args)
                before = metrics(metrics_port)
                runs = []
                for _ in range(args.repeats):
                    cpu = tree_cpu_seconds(server.proc.pid)
                    run = ab(url, args.requests, args.concurrency, headers, *post_args)
                    run["server_cpu_ms_per_request"] = (tree_cpu_seconds(server.proc.pid) - cpu) / args.requests * 1000
                    runs.append(run)
                summary = {k: statistics.median(r[k] for r in runs) for k in ("requests_per_second", "p50_ms", "p95_ms", "p99_ms", "server_cpu_ms_per_request")}
                summary["failed"] = sum(r["failed"] for r in runs)
                summary["status"] = got
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
                    fetches = after["fetch"]["duration_count"] - before["fetch"]["duration_count"]
                    if fetches:
                        summary["fetch_ms_mean"] = (after["fetch"]["duration_seconds_sum"] - before["fetch"]["duration_seconds_sum"]) / fetches * 1000
                load.append({"fixture": fixture, "server": name, "label": label, "path": path, "concurrency": args.concurrency, "median": summary, "runs": runs})
                extra = ""
                if "executor_requests_per_http_request" in summary:
                    extra = f"  js/req {summary['executor_requests_per_http_request']:.2f}  queue {summary['queue_ms_mean']:.2f}  exec {summary['execution_ms_mean']:.2f} ms  heap {summary['v8_heap_used_mib']:.0f} MiB"
                    if "fetch_ms_mean" in summary:
                        extra += f"  fetch {summary['fetch_ms_mean']:.2f} ms"
                print(f"{label:28} {name:10} {summary['requests_per_second']:8.0f} req/s  p50 {summary['p50_ms']:6.1f}  p95 {summary['p95_ms']:6.1f}  p99 {summary['p99_ms']:6.1f} ms  cpu {summary['server_cpu_ms_per_request']:5.2f} ms/req  rss {summary['rss_mib_after']:6.1f} MiB  status {got}  failed {summary['failed']}{extra}", flush=True)
        finally:
            server.stop()
            time.sleep(1)
    return load

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", default="benchmarks/results")
    parser.add_argument("--nextrs-2a", default=None)
    parser.add_argument("--concurrency", type=int, default=32)
    parser.add_argument("--requests", type=int, default=4000)
    parser.add_argument("--repeats", type=int, default=2)
    parser.add_argument("--upstream-port", type=int, default=4986)
    parser.add_argument("--only-fixture", nargs="*", default=None)
    args = parser.parse_args()

    upstream = f"http://127.0.0.1:{args.upstream_port}"
    if not free(args.upstream_port):
        sys.exit(f"port {args.upstream_port} is in use")
    upstream_proc = subprocess.Popen(
        ["node", str(ROOT / "tests/compat/upstream.mjs"), "--port", str(args.upstream_port)],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True,
    )
    secret = base64.urlsafe_b64encode(os.urandom(24)).decode()
    env = {
        "PATH": os.environ["PATH"], "HOME": os.environ["HOME"], "NEXT_TELEMETRY_DISABLED": "1",
        "NEXTRS_UPSTREAM": upstream, "SESSION_SECRET": secret, "NEXTRS_CLOSED_PORT": "9", "NEXTRS_UNLISTED_PORT": "10",
    }
    nextrs = str(ROOT / "target/release/nextrs")
    flags = ["--js-env", "NEXTRS_UPSTREAM", "--js-env", "SESSION_SECRET", "--net-allow", upstream, "--net-allow-addr", "127.0.0.1"]
    body_file = str(ROOT / "target/bench-action-body.txt")
    content_type = action_body(body_file)
    cookie = f"Cookie: session={session_token(secret, 'u-alice', 'Alice')}"

    proxy_dir = ROOT / "fixtures/next-proxy"
    manifest = json.loads((proxy_dir / ".next/server/server-reference-manifest.json").read_text())
    inspect = next(k for k, v in manifest["node"].items() if v["exportedName"] == "inspect" and v["filename"] == "app/actions/actions.ts")

    # (fixture dir, served path, [(label, path, headers, post, include 2a)])
    fixtures = [
        ("next-proxy", proxy_dir, proxy_dir / ".next", [
            ("unmatched prerendered", "/excluded", [], False, True),
            ("matched prerendered", "/target", [], False, True),
            ("proxy redirect", "/proxy/redirect", [], False, True),
            ("proxy rewrite", "/proxy/rewrite", [], False, True),
            ("proxy + ISR", "/isr", [], False, True),
            ("proxy + Server Action", "/actions", [f"Next-Action: {inspect}", "Accept: text/x-component", "Origin: ORIGIN"], True, True),
        ]),
        ("next-auth-proxy", ROOT / "fixtures/next-auth-proxy", ROOT / "fixtures/next-auth-proxy", [
            ("authenticated (jose HS256)", "/account", [cookie], False, False),
            ("invalid-session redirect", "/account", ["Cookie: session=forged.token.value"], False, True),
        ]),
        ("next-platform-proxy", ROOT / "fixtures/next-platform-proxy", ROOT / "fixtures/next-platform-proxy", [
            ("proxy with outbound fetch", "/gate/open", [], False, True),
            ("external rewrite", "/ext/bench", [], False, False),
        ]),
    ]
    if args.only_fixture:
        fixtures = [f for f in fixtures if f[0] in args.only_fixture]

    results = {"environment": environment(), "config": vars(args), "load": []}
    try:
        for fixture, project, serve, paths in fixtures:
            servers = {"next-start": (["npx", "next", "start", "-H", "0.0.0.0", "-p", "9440"], 9440, None)}
            if args.nextrs_2a:
                servers["2a"] = ([str(Path(args.nextrs_2a).resolve()), "start", str(serve), "--port", "9441", "--js-runtime", "embedded-v8", *flags, "--metrics-addr", "127.0.0.1:9451"], 9441, 9451)
            servers["2b"] = ([nextrs, "start", str(serve), "--port", "9442", "--js-runtime", "embedded-v8", *flags, "--metrics-addr", "127.0.0.1:9452"], 9442, 9452)
            servers["2b-i4"] = ([nextrs, "start", str(serve), "--port", "9443", "--js-runtime", "embedded-v8", "--isolates", "4", *flags, "--metrics-addr", "127.0.0.1:9453"], 9443, 9453)
            results["load"] += measure(fixture, project, servers, paths, env, args, body_file, content_type)
    finally:
        os.killpg(upstream_proc.pid, 15)
    out = ROOT / args.out
    out.mkdir(parents=True, exist_ok=True)
    target = out / f"phase2b-{time.strftime('%Y%m%dT%H%M%S')}.json"
    target.write_text(json.dumps(results, indent=2))
    print(f"wrote {target}")


if __name__ == "__main__":
    main()
