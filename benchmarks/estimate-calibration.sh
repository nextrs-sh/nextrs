#!/usr/bin/env bash
# Checks nextrs_image::estimate against measured peaks: every corpus image
# (benchmarks/image-corpus.mjs, plus fixtures/next-image photos) to WebP,
# JPEG, PNG and AVIF at four widths, one fresh process each
# (crates/nextrs-image/examples/measure.rs: Rust heap peak and RSS peak).
#
#   benchmarks/estimate-calibration.sh CORPUS_DIR OUT.jsonl
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
corpus="$1" out="$2"
cargo build --release -p nextrs-image --example measure >&2
m="$root/target/release/examples/measure"
: > "$out"
for f in "$corpus"/*.{jpg,png,webp} "$root"/fixtures/next-image/public/photos/{rotated.jpg,p3.jpg,alpha.webp,still.gif,lzw.tiff}; do
  for w in 64 640 1920 3840; do
    for fmt in webp jpeg png avif; do
      [[ $fmt == avif && $w == 3840 ]] && continue
      "$m" "$f" "$fmt" "$w" 75 1 >> "$out" 2>/dev/null || echo "{\"input\":\"$f\",\"format\":\"$fmt\",\"width\":$w,\"error\":true}" >> "$out"
    done
  done
done
python3 - "$out" <<'PY'
import json, sys
rows = [json.loads(l) for l in open(sys.argv[1])]
ok = [r for r in rows if not r.get("error")]
under = [r for r in ok if r["estimate_mib"] < max(r["peak_heap_mib"], r["peak_rss_delta_mib"] - 3.5)]
ratio = sorted(r["estimate_mib"] / max(r["peak_heap_mib"], r["peak_rss_delta_mib"] - 3.5, 0.5) for r in ok)
print(f"{len(ok)} measured, {len(rows) - len(ok)} not transformable, {len(under)} under-estimated")
for r in under:
    print("  UNDER", r["input"].split("/")[-1], r["format"], r["width"], "heap", r["peak_heap_mib"], "rss", r["peak_rss_delta_mib"], "est", r["estimate_mib"])
print("estimate / measured: min %.2f median %.2f max %.2f" % (ratio[0], ratio[len(ratio) // 2], ratio[-1]))
PY
