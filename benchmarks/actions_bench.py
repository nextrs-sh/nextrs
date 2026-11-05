#!/usr/bin/env python3
"""Phase 1D benchmarks: Server Actions against `next start` and the Phase
1C binary (stdlib only; load: ab).

    python3 benchmarks/actions_bench.py --project fixtures/next-app \
        --nextrs-1c /path/to/phase-1c/nextrs --out benchmarks/results

Servers, all on the same .next build and one upstream
(tests/compat/upstream.mjs):

  next-start   `next start`
  1c           the Phase 1C binary (--allow-unsupported: it refuses builds
               with Server Actions; actions answer 501, so it runs the
               GET paths only, as the regression baseline)
  1d, 1d-i4    this tree, 1 and 4 isolates

Requests (bodies are what Chromium sends for the fixtures; see
tests/browser/actions.mjs):

  GET /books/42, /api/health   SSR and a trivial handler: regression check
  action-small    whoAmI("bench"): a text/plain reply, no page render
  action-form     echoForm via useActionState: a multipart reply, 5 fields
  action-upload   the same with a 256 KiB file
  mpa-form        createBook as a no-JavaScript form post (HTML render)
  mpa-closure     the encrypted closure on /actions/bound, no JavaScript
                  (AES-GCM decryption of its bound arguments, HTML render)

Before measuring, each server's answer to every request is compared with
next start's (status, content type, and the body with digests and
boundaries normalized); a server that differs is not measured. Results go
to <out>/actions-<time>.json.
"""

import argparse
import csv
import io
import json
import os
import re
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
from dynamic_bench import Server, commands  # noqa: E402

UPSTREAM_PORT = 4975
BOUNDARY = "----NextRSBench7d2f"
GET_PATHS = ["/books/42", "/api/health"]


def multipart(fields):
    """fields: (name, value[, filename, type]) -> bytes."""
    out = io.BytesIO()
    for field in fields:
        name, value = field[0], field[1]
        out.write(f"--{BOUNDARY}\r\n".encode())
        if len(field) > 2:
            out.write(f'Content-Disposition: form-data; name="{name}"; filename="{field[2]}"\r\nContent-Type: {field[3]}\r\n\r\n'.encode())
        else:
            out.write(f'Content-Disposition: form-data; name="{name}"\r\n\r\n'.encode())
        out.write(value if isinstance(value, bytes) else value.encode())
        out.write(b"\r\n")
    out.write(f"--{BOUNDARY}--\r\n".encode())
    return out.getvalue()


def action_ids(project):
    manifest = json.loads((project / ".next/server/server-reference-manifest.json").read_text())

    def find(name, filename):
        for key, value in manifest["node"].items():
            if value["exportedName"] == name and value["filename"] == filename:
                return key
        sys.exit(f"no action {name} in {filename}")

    return {
        "who": find("whoAmI", "app/actions/server.ts"),
        "echo": find("echoForm", "app/actions/server.ts"),
        "create": find("createBook", "app/actions/actions.ts"),
    }


def closure_fields(port):
    """The hidden inputs of /actions/bound's closure form (encrypted)."""
    html = fetch(port, "GET", "/actions/bound")[2].decode()
    start = html.index('id="closure-form"')
    form = html[html.rindex("<form", 0, start): html.index("</form>", start)]
    fields = []
    for attrs in re.findall(r"<input([^>]*)>", form):
        found = dict(re.findall(r'([a-zA-Z-]+)="([^"]*)"', attrs))
        if "name" in found:
            value = found.get("value", "").replace("&quot;", '"').replace("&amp;", "&")
            fields.append((found["name"], value))
    return fields


def requests_for(ids, port):
    form_fields = [
        ("_1_$ACTION_REF_1", ""),
        ("_1_$ACTION_1:0", json.dumps({"id": ids["echo"], "bound": "$@1"}, separators=(",", ":"))),
        ("_1_$ACTION_1:1", "[null]"),
        ("_1_title", "Dune"),
        ("_1_author", "Frank Herbert"),
        ("_1_note", "bench"),
        ("_1_empty", ""),
        ("0", '[null,"$K1"]'),
    ]
    upload = bytes((i * 31 + 7) % 256 for i in range(256 * 1024))
    mp = f"multipart/form-data; boundary={BOUNDARY}"
    return {
        "action-small": ("/actions/client", {"next-action": ids["who"], "accept": "text/x-component"}, "text/plain;charset=UTF-8", b'["bench",0]'),
        "action-form": ("/actions/form", {"next-action": ids["echo"], "accept": "text/x-component"}, mp, multipart(form_fields)),
        "action-upload": ("/actions/form", {"next-action": ids["echo"], "accept": "text/x-component"}, mp,
                          multipart(form_fields[:-1] + [("_1_file", upload, "cover.bin", "application/octet-stream"), form_fields[-1]])),
        "mpa-form": ("/actions/simple", {}, mp, multipart([(f"$ACTION_ID_{ids['create']}", ""), ("title", "Dune")])),
        "mpa-closure": ("/actions/bound", {}, mp, multipart([(n, "Lin" if n == "name" else v) for n, v in closure_fields(port)])),
    }


def fetch(port, method, path, headers=None, body=None):
    request = urllib.request.Request(f"http://127.0.0.1:{port}{path}", data=body, method=method, headers={"accept-encoding": "identity", **(headers or {})})
    try:
        with urllib.request.urlopen(request) as response:
            return response.status, response.headers.get("content-type", ""), response.read()
    except urllib.error.HTTPError as error:
        return error.code, error.headers.get("content-type", ""), error.read()


def normalize(body):
    text = body.decode("utf-8", "replace")
    text = re.sub(r'"digest":"[^"]*"', '"digest":"<d>"', text)
    # Per-render nonces Next puts in HTML: none in this fixture, but keep
    # the comparison about semantics, not chunk boundaries.
    return text


def run_ab(url, requests, concurrency, method="GET", headers=None, content_type=None, body_file=None):
    with tempfile.NamedTemporaryFile("r", suffix=".csv") as percentiles:
        cmd = ["ab", "-q", "-k", "-n", str(requests), "-c", str(concurrency), "-e", percentiles.name]
        for name, value in (headers or {}).items():
            cmd += ["-H", f"{name}: {value}"]
        if method == "POST":
            cmd += ["-p", body_file, "-T", content_type]
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


def heap(metrics_port):
    if not metrics_port:
        return None
    with urllib.request.urlopen(f"http://127.0.0.1:{metrics_port}/metrics.json") as response:
        snapshot = json.loads(response.read())
    return sum(e.get("heap_used_bytes", 0) for e in snapshot.get("executors", [])) / 2**20


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--project", default="fixtures/next-app")
    parser.add_argument("--out", default="benchmarks/results")
    parser.add_argument("--nextrs-1c", default=None)
    parser.add_argument("--concurrency", type=int, nargs="+", default=[1, 8, 32, 64, 128])
    parser.add_argument("--requests", type=int, default=2000)
    parser.add_argument("--repeats", type=int, default=2)
    parser.add_argument("--only", nargs="*", default=None)
    parser.add_argument("--paths", nargs="*", default=None)
    args = parser.parse_args()

    project = (ROOT / args.project).resolve()
    upstream = f"http://127.0.0.1:{UPSTREAM_PORT}"
    base = commands(project, 0, None)
    next_cmd, next_cwd, next_env = base["next-start"]
    rs_cmd, rs_cwd, rs_env = base["nextrs-v8"]
    next_env = {**next_env, "NEXTRS_UPSTREAM": upstream}
    rs_env = {**rs_env, "NEXTRS_UPSTREAM": upstream}
    net = ["--js-env", "NEXTRS_UPSTREAM", "--net-allow", upstream, "--net-allow-addr", "127.0.0.1"]
    variants = {"next-start": ((next_cmd, next_cwd, next_env), None, True)}
    if args.nextrs_1c:
        variants["1c"] = (([args.nextrs_1c] + rs_cmd[1:] + net + ["--allow-unsupported"], rs_cwd, rs_env), 9401, False)
    variants["1d"] = ((rs_cmd + net, rs_cwd, rs_env), 9402, True)
    variants["1d-i4"] = ((rs_cmd + net + ["--isolates", "4"], rs_cwd, rs_env), 9403, True)
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

    with socket.socket() as s:
        if s.connect_ex(("127.0.0.1", UPSTREAM_PORT)) == 0:
            sys.exit(f"port {UPSTREAM_PORT} is in use")
    upstream_proc = subprocess.Popen(["node", str(ROOT / "tests/compat/upstream.mjs"), "--port", str(UPSTREAM_PORT)],
                                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
    time.sleep(0.5)
    ids = action_ids(project)
    ports = {name: 3800 + i for i, name in enumerate(variants)}
    results = {"environment": environment(), "config": {**vars(args), "nextrs_1c": Path(args.nextrs_1c).name if args.nextrs_1c else None}, "load": []}
    reference = {}
    bodies = tempfile.mkdtemp(prefix="nextrs-actions-bench-")
    try:
        for name, (_, metrics_port, actions) in variants.items():
            server = make(name, ports[name])
            try:
                server.start()
                pid = server.proc.pid
                plan = [("GET", path, None) for path in GET_PATHS]
                if actions:
                    for label, (path, headers, content_type, body) in requests_for(ids, server.port).items():
                        headers = {**headers, "origin": f"http://127.0.0.1:{server.port}"}
                        status, ctype, answer = fetch(server.port, "POST", path, {**headers, "content-type": content_type}, body)
                        if status != 200:
                            sys.exit(f"{name}: {label} answered {status}; refusing to benchmark")
                        semantic = (status, ctype.split(";")[0], len(normalize(answer)) if ctype.startswith("text/html") else normalize(answer))
                        if label in reference and reference[label] != semantic:
                            sys.exit(f"{name}: {label} differs from next start; refusing to benchmark:\n{reference[label]}\n{semantic}")
                        reference.setdefault(label, semantic)
                        body_file = os.path.join(bodies, f"{name}-{label}.bin")
                        Path(body_file).write_bytes(body)
                        plan.append(("POST", path, (label, headers, content_type, body_file, len(body))))
                if args.paths:
                    plan = [p for p in plan if (p[2][0] if p[2] else p[1]) in args.paths]
                for method, path, post in plan:
                    label = post[0] if post else path
                    requests = args.requests if not post or post[4] < 100_000 else max(args.requests // 4, 200)
                    for concurrency in args.concurrency:
                        url = f"http://127.0.0.1:{server.port}{path}"
                        kw = {"method": method}
                        if post:
                            kw.update(headers=post[1], content_type=post[2], body_file=post[3])
                        run_ab(url, max(requests // 8, 2 * concurrency), concurrency, **kw)
                        runs = []
                        for _ in range(args.repeats):
                            cpu = tree_cpu_seconds(pid)
                            measured = run_ab(url, requests, concurrency, **kw)
                            measured["server_cpu_ms_per_request"] = (tree_cpu_seconds(pid) - cpu) / requests * 1000
                            measured["rss_mib_after"] = tree_rss_kib(pid) / 1024
                            runs.append(measured)
                        summary = {k: statistics.median(r[k] for r in runs) for k in ("requests_per_second", "p50_ms", "p95_ms", "p99_ms", "server_cpu_ms_per_request")}
                        summary["rss_mib_after"] = runs[-1]["rss_mib_after"]
                        summary["failed"] = sum(r["failed"] for r in runs)
                        summary["v8_heap_used_mib"] = heap(metrics_port)
                        if post:
                            summary["upload_mib_per_second"] = summary["requests_per_second"] * post[4] / 2**20
                        results["load"].append({"server": name, "request": label, "concurrency": concurrency, "median": summary, "runs": runs})
                        heap_text = f"  heap {summary['v8_heap_used_mib']:6.1f} MiB" if summary["v8_heap_used_mib"] is not None else ""
                        print(f"{label:14} c={concurrency:<3} {name:10} {summary['requests_per_second']:8.0f} req/s  p50 {summary['p50_ms']:7.2f}  p95 {summary['p95_ms']:7.2f}  p99 {summary['p99_ms']:7.2f} ms  cpu {summary['server_cpu_ms_per_request']:5.2f} ms/req  rss {summary['rss_mib_after']:6.1f} MiB{heap_text}  failed {summary['failed']}", flush=True)
            finally:
                server.stop()
    finally:
        os.killpg(upstream_proc.pid, 15)

    out = ROOT / args.out
    out.mkdir(parents=True, exist_ok=True)
    path = out / f"actions-{time.strftime('%Y%m%dT%H%M%S')}.json"
    path.write_text(json.dumps(results, indent=2))
    print(f"wrote {path}")


if __name__ == "__main__":
    main()
