#!/usr/bin/env bash
# Entry point of a clean-install container. Runs as root in a bare
# distribution image whose only mount is the bundle made by
# scripts/clean-install-bundle.sh (read-only, at $BUNDLE, default /bundle).
#
# 1. installs curl and nothing else (no recommended packages, so no
#    ca-certificates: outbound TLS must use NextRS's bundled roots);
# 2. records and checks the isolation: no Rust or Node.js toolchain, no
#    Cargo registry, no NextRS checkout, and none of the build-machine
#    source paths recorded in the binary exists;
# 3. runs release-smoke.sh and clean-install-suite.sh (1 and 4 isolates)
#    as an unprivileged user, writing JSON results to $RESULTS (default
#    /results, which may be the one writable mount).
set -uo pipefail
BUNDLE=${BUNDLE:-/bundle}
RESULTS=${RESULTS:-/results}
mkdir -p "$RESULTS"
image=$(. /etc/os-release; echo "${ID}-${VERSION_ID}")
log="$RESULTS/$image.log"
exec > >(tee "$log") 2>&1

echo "== image: $(. /etc/os-release; echo "$PRETTY_NAME"), glibc $(ldd --version | head -1 | awk '{print $NF}')"
if ! command -v curl > /dev/null; then
  apt-get update -qq && DEBIAN_FRONTEND=noninteractive apt-get install -y -qq --no-install-recommends curl > /dev/null || exit 1
fi

fail=0
iso() { printf '%-28s %s  %s\n' "$1" "$2" "$3"; [[ $2 == pass ]] || fail=1; echo "{\"check\":\"$1\",\"result\":\"$2\",\"detail\":\"$3\"}" >> "$RESULTS/$image.isolation.jsonl"; }
: > "$RESULTS/$image.isolation.jsonl"

found=""
for t in cargo rustc rustup node nodejs npm deno bun; do command -v "$t" > /dev/null 2>&1 && found+=" $t"; done
iso no-toolchain "$([[ -z $found ]] && echo pass || echo fail)" "on PATH:${found:- none of cargo rustc rustup node nodejs npm deno bun}"
dirs=$(find / -xdev \( -path /proc -o -path /sys \) -prune -o -type d \( -name '.cargo' -o -name '.rustup' -o -name 'registry' -path '*cargo*' -o -name 'deno_web-*' -o -name 'deno_webidl-*' -o -name 'deno_core-*' -o -name 'rusty_v8*' \) -print 2>/dev/null | head -5)
iso no-cargo-registry "$([[ -z $dirs ]] && echo pass || echo fail)" "${dirs:-no .cargo, .rustup, Cargo registry or deno crate directory on the root filesystem}"
checkout=$(find / -xdev \( -path /proc -o -path /sys \) -prune -o -name Cargo.toml -print 2>/dev/null | head -3)
iso no-source-checkout "$([[ -z $checkout ]] && echo pass || echo fail)" "${checkout:-no Cargo.toml anywhere on the root filesystem}"
mounts=$(awk '{print $5}' /proc/self/mountinfo | grep -Ev '^/(proc|sys|dev)(/|$)' | sort -u | tr '\n' ' ')
iso mounts pass "$mounts"

tmp=$(mktemp -d)
tar -xzf "$BUNDLE"/nextrs-v*-linux-x86_64.tar.gz -C "$tmp"
bin=$(echo "$tmp"/nextrs-v*/nextrs)
# The paths are stored back to back, without separators: split after ".js".
recorded=$(grep -aoE '/[A-Za-z0-9_./-]*deno_(web|webidl)-[0-9.]+/[A-Za-z0-9_]+\.js' "$bin" | sed 's#\.js/#.js\n/#g' | grep -E '/deno_(web|webidl)-[0-9.]+/[A-Za-z0-9_]+\.js$' | sort -u)
present=""
for p in $recorded; do [[ -e $p ]] && present+=" $p"; done
iso build-paths-absent "$([[ -z $present ]] && echo pass || echo fail)" "$(echo "$recorded" | grep -c . ) build-machine deno_web/deno_webidl paths recorded in the binary; present here:${present:- none}"
rm -rf "$tmp"

useradd --create-home --shell /bin/bash nextrs-test 2>/dev/null || true
chown nextrs-test "$RESULTS"
run() { su nextrs-test -s /bin/bash -c "cd ~ && $*"; }
archive=$(ls "$BUNDLE"/nextrs-v*-linux-x86_64.tar.gz)
# release-smoke.sh checks the archive's .sha256 next to it: copy both.
run "mkdir -p dl && cp '$archive' '$archive.sha256' dl/"
run "bash '$BUNDLE/scripts/release-smoke.sh' --archive ~/dl/$(basename "$archive") --app '$BUNDLE/apps/next-app' --json '$RESULTS/$image.smoke.json'" || fail=1
for n in 1 4; do
  run "bash '$BUNDLE/scripts/clean-install-suite.sh' --archive ~/dl/$(basename "$archive") --bundle '$BUNDLE' --isolates $n --json '$RESULTS/$image.suite-i$n.json'" || fail=1
done
echo "== $image: $([[ $fail == 0 ]] && echo PASS || echo FAIL)"
exit $fail
