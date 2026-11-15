#!/usr/bin/env bash
# Builds nextrs with release-profile variants for the Phase 3A build
# experiments, each in its own target directory, and records build time,
# binary size and startup time:
#
#   benchmarks/phase3a/build-variants.sh [variant ...]
#
# Variants (Cargo --config overrides; Cargo.toml is not changed):
#   default        Cargo's release profile (opt-level 3, no LTO, 16 CGUs)
#   thin-lto       lto = "thin"
#   fat-lto-cgu1   lto = "fat", codegen-units = 1
#   cgu1           codegen-units = 1
#   x86-64-v3      default profile, -C target-cpu=x86-64-v3 (AVX2 hosts
#                  only: never a default for portable binaries)
# V8 is a prebuilt static library: these settings change the Rust code
# around it. Binaries go to target/bench-bins/nextrs-variant-<name>; a
# summary to target/bench-bins/variants.tsv.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$root"
variants=("$@")
[[ ${#variants[@]} -gt 0 ]] || variants=(default thin-lto fat-lto-cgu1 cgu1 x86-64-v3)
mkdir -p target/bench-bins
# Reuse the prebuilt V8 archive already fetched for target/release instead
# of downloading it again for every target directory (the archive does not
# depend on these settings). It is copied out first: pointing
# RUSTY_V8_ARCHIVE at a target's own gn_out copy makes the build script
# copy the file onto itself, which truncates it.
if [[ -z ${RUSTY_V8_ARCHIVE:-} && -s target/release/gn_out/obj/librusty_v8.a ]]; then
  mkdir -p target/v8-archive
  cp target/release/gn_out/obj/librusty_v8.a target/v8-archive/librusty_v8.a
  export RUSTY_V8_ARCHIVE="$root/target/v8-archive/librusty_v8.a"
fi
out=target/bench-bins/variants.tsv
[[ -f $out ]] || printf 'variant\tcommit\tbuild_seconds\tbinary_bytes\tstripped_bytes\tstartup_ms_median\n' > "$out"
for v in "${variants[@]}"; do
  config=() rustflags=""
  case "$v" in
    default) ;;
    thin-lto) config=(--config 'profile.release.lto="thin"') ;;
    fat-lto-cgu1) config=(--config 'profile.release.lto="fat"' --config 'profile.release.codegen-units=1') ;;
    cgu1) config=(--config 'profile.release.codegen-units=1') ;;
    x86-64-v3) rustflags="-C target-cpu=x86-64-v3" ;;
    *) echo "unknown variant $v" >&2; exit 2 ;;
  esac
  dir="target/variant-$v"
  echo "== $v"
  start=$(date +%s)
  # A clean build of the workspace crates (dependencies are rebuilt too:
  # LTO and codegen settings apply to them).
  RUSTFLAGS="$rustflags" CARGO_TARGET_DIR="$dir" cargo build --release -p nextrs-cli "${config[@]}" 2>&1 | tail -1
  seconds=$(( $(date +%s) - start ))
  bin="target/bench-bins/nextrs-variant-$v"
  cp "$dir/release/nextrs" "$bin"
  size=$(stat -c %s "$bin")
  strip -o "$bin.stripped" "$bin" && stripped=$(stat -c %s "$bin.stripped") && rm -f "$bin.stripped" || stripped=""
  # Startup: time until a prerendered page answers (fixtures/next-app with
  # one V8 isolate, as served in the benchmarks), median of 5. Gives up
  # after 30 s or if the server exits.
  times=()
  for _ in 1 2 3 4 5; do
    t0=$(date +%s%N)
    "$bin" start fixtures/next-app --port 6199 --js-runtime embedded-v8 --isolates 1 > /dev/null 2>&1 &
    pid=$!
    deadline=$(( $(date +%s) + 30 ))
    until curl -s -o /dev/null http://127.0.0.1:6199/; do
      if ! kill -0 $pid 2>/dev/null || (( $(date +%s) > deadline )); then
        echo "   startup check failed for $v" >&2; kill $pid 2>/dev/null || true; exit 1
      fi
      sleep 0.005
    done
    times+=($(( ($(date +%s%N) - t0) / 1000000 )))
    kill $pid; wait $pid 2>/dev/null || true
  done
  median=$(printf '%s\n' "${times[@]}" | sort -n | sed -n 3p)
  printf '%s\t%s\t%s\t%s\t%s\t%s\n' "$v" "$(git rev-parse --short HEAD)" "$seconds" "$size" "$stripped" "$median" >> "$out"
  echo "   ${seconds}s build, $size bytes ($stripped stripped), startup ${median} ms"
done
column -t "$out"
