#!/usr/bin/env bash
# fetch.sh OUTDIR : license files of rusty_v8 v150.4.0 and the C/C++ sources it links, at pinned revisions.
set -euo pipefail
OUT=$1; mkdir -p "$OUT"; : > "$OUT/SOURCES.tsv"
gs() { # component repo sha
  local c=$1 repo=$2 sha=$3
  mkdir -p "$OUT/$c"
  local list
  list=$(curl -sSf "https://chromium.googlesource.com/$repo/+/$sha/?format=JSON" | tail -n +2 | python3 -c 'import json,sys,re; print("\n".join(e["name"] for e in json.load(sys.stdin)["entries"] if re.match(r"(?i)(licen[cs]e|copying|notice)", e["name"]) and e["type"]=="blob"))')
  [[ -n $list ]] || { echo "no license file in $repo@$sha" >&2; exit 1; }
  for f in $list; do
    curl -sSf "https://chromium.googlesource.com/$repo/+/$sha/$f?format=TEXT" | base64 -d > "$OUT/$c/$f"
    printf '%s\t%s\t%s\t%s\n' "$c" "https://chromium.googlesource.com/$repo" "$sha" "$f" >> "$OUT/SOURCES.tsv"
  done
}
gh_raw() { # component owner/repo ref files...
  local c=$1 repo=$2 ref=$3; shift 3
  mkdir -p "$OUT/$c"
  for f in "$@"; do
    curl -sSfL "https://raw.githubusercontent.com/$repo/$ref/$f" > "$OUT/$c/$f"
    printf '%s\t%s\t%s\t%s\n' "$c" "https://github.com/$repo" "$ref" "$f" >> "$OUT/SOURCES.tsv"
  done
}
gh_raw rusty_v8 denoland/rusty_v8 5c15a6995c9bb4bacd3e341b59fff32c909c80bf LICENSE
gh_raw v8 denoland/v8 ac1e23989121713ca642f6650b34deff7b686896 LICENSE LICENSE.fdlibm LICENSE.strongtalk LICENSE.v8
gh_raw fp16 Maratyszcza/FP16 3d2de1816307bac63c16a297e8c4dc501b4076df LICENSE
gs icu chromium/deps/icu ee5f27adc28bd3f15b2c293f726d14d2e336cbd5
gs abseil-cpp chromium/src/third_party/abseil-cpp d16e32215c3ab90ba57c2e904a5344d85c7353e4
gs libc++ external/github.com/llvm/llvm-project/libcxx 5abc7f839700f0f17338434e1c1c6a8c87c00c11
gs libc++abi external/github.com/llvm/llvm-project/libcxxabi 8f11bb1d4438d0239d0dfc1bd9456a9f31629dda
gs libunwind external/github.com/llvm/llvm-project/libunwind d6c7a21e978f0adaa43accaad53bc64f0b64f6ec
gs llvm-libc external/github.com/llvm/llvm-project/libc 9309c117ebae84dd2f9df1ef99de4782162527d5
gs fast_float external/github.com/fastfloat/fast_float 05087a303dad9c98768b33c829d398223a649bc6
gs simdutf chromium/src/third_party/simdutf f7356eed293f8208c40b3c1b344a50bd70971983
gs highway external/github.com/google/highway 2607d3b5b0113992fe84d3848859eae13b3b52c1
# partition_alloc (ff3b8b88) has no license file of its own: its sources are
# "governed by a BSD-style license that can be found in the LICENSE file" of
# Chromium, which is fetched here at a fixed Chromium revision.
gh_raw partition_alloc chromium/chromium 7d14fca94db5b35539e3c28fa308dfd97022238b LICENSE
gs dragonbox external/github.com/jk-jeon/dragonbox beeeef91cf6fef89a4d4ba5e95d47ca64ccb3a44
for id in MIT BSD-3-Clause Apache-2.0; do
  mkdir -p "$OUT/spdx"; curl -sSfL "https://raw.githubusercontent.com/spdx/license-list-data/v3.27.0/text/$id.txt" > "$OUT/spdx/$id.txt"
  printf '%s\t%s\t%s\t%s\n' spdx https://github.com/spdx/license-list-data v3.27.0 "text/$id.txt" >> "$OUT/SOURCES.tsv"
done
