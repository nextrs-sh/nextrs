#!/usr/bin/env bash
# Runs benchmarks/image_bench.mjs and benchmarks/image_faults.mjs against one
# server on fixtures/next-image (default build, image upstream port 4317),
# each from an empty image cache:
#
#   benchmarks/image-bench.sh --server next|PATH_TO_NEXTRS --label NAME --out DIR
#                             [--isolates N] [-- EXTRA_NEXTRS_FLAGS...]
#
# --server next runs next start (its .next/cache/images is cleared first);
# otherwise the given NextRS binary (memory cache only). The faults script
# needs NextRS's metrics, so it runs only for NextRS; the 30-transformation
# burst is also measured on next start.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
project="$root/fixtures/next-image"
server="" label="" out="" isolates=1 extra=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --server) server="$2"; shift 2 ;;
    --label) label="$2"; shift 2 ;;
    --out) out="$2"; shift 2 ;;
    --isolates) isolates="$2"; shift 2 ;;
    --) shift; extra=("$@"); break ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
[[ -n $server && -n $label && -n $out ]] || { sed -n 2,13p "$0"; exit 2; }
mkdir -p "$out"
out="$(cd "$out" && pwd)"
port=5900 metrics_port=5901
export NEXTRS_IMAGE_UPSTREAM_PORT=4317

if ! grep -q '"4317"' "$project/.next/images-manifest.json" 2>/dev/null; then
  (cd "$project" && npm ci --no-audit --no-fund --loglevel=error && node scripts/make-images.mjs >/dev/null &&
    rm -rf .next && NEXT_TELEMETRY_DISABLED=1 npx next build > "$out/build.log" 2>&1)
fi

listener_pid() { ss -ltnpH "sport = :$1" | sed -n 's/.*pid=\([0-9]*\).*/\1/p' | head -1; }
stop() {
  for p in $port $metrics_port; do
    local pid; pid="$(listener_pid $p)"
    [[ -n $pid ]] && kill "$pid" 2>/dev/null || true
  done
  for _ in $(seq 1 40); do [[ -z "$(listener_pid $port)" ]] && return; sleep 0.25; done
}
start() {
  stop
  if [[ $server == next ]]; then
    rm -rf "$project/.next/cache/images"
    (cd "$project" && NEXT_TELEMETRY_DISABLED=1 exec setsid npx next start -H 127.0.0.1 -p $port) >> "$out/$label-server.log" 2>&1 &
  else
    "$server" start "$project/.next" --port $port --js-runtime embedded-v8 --isolates "$isolates" \
      --metrics-addr 127.0.0.1:$metrics_port \
      --net-allow "http://127.0.0.1:4317" --net-allow-addr 127.0.0.1 "${extra[@]}" >> "$out/$label-server.log" 2>&1 &
  fi
  for _ in $(seq 1 120); do curl -s --max-time 2 -o /dev/null "http://127.0.0.1:$port/" && return; sleep 0.25; done
  echo "server did not start" >&2; exit 1
}
trap stop EXIT

start
node "$root/benchmarks/image_bench.mjs" --server "http://127.0.0.1:$port" --label "$label" --project "$project" \
  --upstream-port 4317 --out "$out/$label-bench.json" > "$out/$label-bench.txt"
start
if [[ $server == next ]]; then
  node -e '
    const http = require("node:http"), { execSync } = require("node:child_process"), fs = require("node:fs");
    const port = process.argv[1];
    const rss = () => Number(/VmRSS:\s+(\d+)/.exec(fs.readFileSync(`/proc/${/pid=(\d+)/.exec(execSync(`ss -ltnpH "sport = :${port}"`).toString())[1]}/status`, "utf8"))[1]) / 1024;
    const keys = [640, 750, 828, 1080, 1200, 1920, 2048, 3840, 384, 256].flatMap((w) => [50, 75, 90].map((q) => `/_next/image?url=%2Fphotos%2Flarge.jpg&w=${w}&q=${q}`));
    const before = rss(); let peak = before; const t = setInterval(() => (peak = Math.max(peak, rss())), 50); const t0 = Date.now();
    Promise.all(keys.map((k) => new Promise((r) => http.get({ host: "127.0.0.1", port, path: k, headers: { accept: "image/webp" } }, (res) => { res.resume(); res.on("end", () => r(res.statusCode)); }).on("error", () => r(0))))).then((s) => {
      clearInterval(t);
      const o = { burstNextStart: { requests: keys.length, seconds: (Date.now() - t0) / 1000, statuses: [...new Set(s)], rssBeforeMiB: Math.round(before), peakRssMiB: Math.round(peak) } };
      console.log(JSON.stringify(o, null, 1)); fs.writeFileSync(process.argv[2], JSON.stringify(o, null, 2));
    });' $port "$out/$label-faults.json" > "$out/$label-faults.txt"
else
  node "$root/benchmarks/image_faults.mjs" --server "http://127.0.0.1:$port" --metrics "http://127.0.0.1:$metrics_port" \
    --out "$out/$label-faults.json" > "$out/$label-faults.txt"
fi
echo "done: $out/$label-bench.json $out/$label-faults.json"
