#!/usr/bin/env bash
# Entry point of an npm installation test container (see
# scripts/npm-install-containers.sh). Runs as root in a bare distribution
# image; /bundle (read-only) is from scripts/npm-install-bundle.sh, /work is
# a volume kept between the phases of one image, /results is writable.
#
#   npm-install-entry.sh deps         network on: install the application's own
#                                     dependencies with npm (npm ci) and pnpm
#   npm-install-entry.sh install      network off: isolation checks, then as an
#                                     unprivileged user, for npm and pnpm: install
#                                     the NextRS tarball, next build, serve and
#                                     probe (npm-install-suite.sh); install with
#                                     lifecycle scripts disabled; one-off npx;
#                                     failure cases (corrupted, tampered, truncated,
#                                     wrong permissions, missing binary, forced
#                                     unsupported cpu/os/libc)
#   npm-install-entry.sh missing-lib  network off, after install: remove libgcc_s
#                                     and run the installed command
#   npm-install-entry.sh old-glibc    network off, a glibc < 2.35 image: install
#                                     (it is allowed) and run (it must fail clearly)
#   npm-install-entry.sh musl         network off, a musl (Alpine) image with its
#                                     own Node.js: install must be refused
set -uo pipefail
MODE=${1:?mode}
B=/bundle R=/results
image=$(. /etc/os-release; echo "${ID}-${VERSION_ID}")
mkdir -p "$R"
exec > >(tee -a "$R/$image.$MODE.log") 2>&1
TGZ=$(ls $B/pkg/nextrs-sh-cli-*.tgz)
EXPECT=$(sed -n 's/.*"binary_sha256": *"\([0-9a-f]*\)".*/\1/p' $B/pkg/release.manifest.json)
VERSION=$(sed -n 's/.*"version": *"\([^"]*\)".*/\1/p' $B/pkg/release.manifest.json | head -1)
export NEXT_TELEMETRY_DISABLED=1 npm_config_update_notifier=false npm_config_fund=false npm_config_audit=false
if [[ $MODE != musl ]]; then export PATH=$B/node/bin:$B/bin:/usr/sbin:/usr/bin:/sbin:/bin; else export PATH=$B/bin:$PATH; fi
fail=0
JL="$R/$image.$MODE.jsonl"
: > "$JL"
rec() { # check, status (0 pass, "info" = recorded, not a gate), detail
  local s=pass
  if [[ $2 == info ]]; then s=info; elif [[ $2 != 0 ]]; then s=fail; fail=1; fi
  printf '%-32s %s  %s\n' "$1" "$s" "$(printf '%s' "$3" | head -c 220 | tr '\n' ' ')"
  node -e 'console.log(JSON.stringify({check:process.argv[1],result:process.argv[2],detail:process.argv[3].slice(0,800)}))' "$1" "$s" "$3" >> "$JL" 2>/dev/null ||
    echo "{\"check\":\"$1\",\"result\":\"$s\"}" >> "$JL"
}
user() { # creates the unprivileged user (the same uid in every phase)
  # The home directory (npm and pnpm caches) is on /work, which persists
  # between the phases, as on a developer machine that goes offline.
  mkdir -p /work
  if command -v useradd > /dev/null; then useradd -u 1500 -d /work/home -m -s /bin/bash dev 2> /dev/null || useradd -u 1500 -d /work/home -s /bin/bash dev 2> /dev/null
  else adduser -D -u 1500 -h /tmp/home-dev -s /bin/sh dev; fi
  id dev > /dev/null || { echo "could not create the test user" >&2; exit 1; }
  mkdir -p "$(awk -F: '$1=="dev"{print $6}' /etc/passwd)" && chown 1500 /work "$(awk -F: '$1=="dev"{print $6}' /etc/passwd)" "$R"
}
as() { su dev -s /bin/bash -c "export PATH='$PATH' NEXT_TELEMETRY_DISABLED=1 npm_config_update_notifier=false npm_config_fund=false npm_config_audit=false npm_config_store_dir=/work/pnpm-store; cd ~ && $*"; }
offline() { # the network is really off
  ! node -e 'fetch("https://registry.npmjs.org/",{signal:AbortSignal.timeout(5000)}).then(()=>process.exit(0),()=>process.exit(1))' 2>/dev/null
}
echo "== $MODE on $(. /etc/os-release; echo "$PRETTY_NAME"), $(getconf GNU_LIBC_VERSION 2>/dev/null || echo 'libc: not glibc'), node $(node --version 2>/dev/null)"

case $MODE in
deps)
  user
  for pm in npm pnpm; do
    as "rm -rf /work/$pm-app && mkdir -p /work/$pm-app && cp -r $B/app/. /work/$pm-app/"
  done
  as "cd /work/npm-app && npm ci --no-audit --no-fund --loglevel=error" > "$R/$image.deps-npm.log" 2>&1
  rec deps:npm $? "npm ci: $(tail -2 "$R/$image.deps-npm.log" | tr '\n' ' ')"
  # pnpm resolves from package.json (exact versions pinned); its lockfile is written here.
  as "cd /work/pnpm-app && rm -f package-lock.json && pnpm install --config.confirmModulesPurge=false" > "$R/$image.deps-pnpm.log" 2>&1
  rec deps:pnpm $? "pnpm install: $(tail -2 "$R/$image.deps-pnpm.log" | tr '\n' ' ')"
  ;;

install)
  user
  found=""
  for t in cargo rustc rustup cc gcc clang make python3 node-gyp; do command -v "$t" > /dev/null 2>&1 && found+=" $t"; done
  rec iso:no-toolchain "$([[ -z $found ]]; echo $?)" "on PATH:${found:- none of cargo rustc rustup cc gcc clang make python3 node-gyp}; node $(node --version), npm $(npm --version), pnpm $(pnpm --version)"
  dirs=$(find / -xdev \( -path /proc -o -path /sys -o -path /work -o -path /bundle \) -prune -o -type d \( -name .cargo -o -name .rustup \) -print 2>/dev/null | head -3)
  rec iso:no-cargo-home "$([[ -z $dirs ]]; echo $?)" "${dirs:-no .cargo or .rustup directory}"
  offline; rec iso:network-off $? "registry.npmjs.org unreachable from this container"

  for pm in npm pnpm; do
    port=$([[ $pm == npm ]] && echo 6300 || echo 6310)
    as "PNPM=pnpm bash $B/scripts/npm-install-suite.sh --pm $pm --project /work/$pm-app --tarball $TGZ --expect-sha256 $EXPECT --probe $B/scripts/probe.mjs --port $port --json $R/$image.suite-$pm.json" || fail=1
  done

  # Lifecycle scripts disabled; and a fresh project with nothing else in it.
  as "mkdir -p /tmp/is-npm && cd /tmp/is-npm && npm init -y > /dev/null && npm install --offline --ignore-scripts -D $TGZ > /dev/null 2>&1 && ./node_modules/.bin/nextrs --version" > /tmp/o 2>&1
  rec ignore-scripts:npm "$([[ $(tail -1 /tmp/o) == "nextrs $VERSION" ]]; echo $?)" "npm install --ignore-scripts: $(tail -1 /tmp/o)"
  as "mkdir -p /tmp/is-pnpm && cd /tmp/is-pnpm && echo '{\"name\":\"p\",\"version\":\"1.0.0\",\"private\":true}' > package.json && pnpm add --offline --ignore-scripts -D $TGZ > /dev/null 2>&1 && ./node_modules/.bin/nextrs --version" > /tmp/o 2>&1
  rec ignore-scripts:pnpm "$([[ $(tail -1 /tmp/o) == "nextrs $VERSION" ]]; echo $?)" "pnpm add --ignore-scripts: $(tail -1 /tmp/o)"

  # One-off execution without adding a dependency.
  as "mkdir -p /tmp/oneoff && cd /tmp/oneoff && npx --yes --offline --package=$TGZ nextrs --version" > /tmp/o 2>&1
  rec one-off:npx "$([[ $(tail -1 /tmp/o) == "nextrs $VERSION" ]]; echo $?)" "npx --yes --package=<package> nextrs --version: $(tail -1 /tmp/o)"
  as "cd /tmp/oneoff && pnpm --package=$TGZ dlx nextrs --version" > /tmp/o 2>&1
  rec one-off:pnpm-dlx "$([[ $(tail -1 /tmp/o) == "nextrs $VERSION" ]]; echo $?)" "pnpm --package=<package> dlx nextrs --version: $(tail -1 /tmp/o)"

  # ----- failure cases, each in its own small project
  # A modified binary repacked under the same name: the lockfile's integrity refuses it.
  as "set -e; mkdir -p /tmp/tamper/src && cd /tmp/tamper && cp $TGZ ./pkg.tgz && npm init -y > /dev/null && npm install --offline -D ./pkg.tgz > /dev/null 2>&1
      tar -xzf pkg.tgz -C src && printf 'X' | dd of=src/package/native/nextrs bs=1 seek=4096 conv=notrunc 2> /dev/null
      tar -czf pkg.tgz -C src package && rm -rf node_modules && npm ci --offline --cache /tmp/tamper/empty-cache > ci.log 2>&1 && echo exit=0 || echo exit=1" > /tmp/o 2>&1
  rec corrupt:tampered-npm "$(! grep -q 'exit=0' /tmp/o && grep -q EINTEGRITY /tmp/tamper/ci.log; echo $?)" "npm ci (cold cache; a warm cache serves the original, verified bytes) after the tarball changed: $(grep -m1 -E 'EINTEGRITY|integrity' /tmp/tamper/ci.log | head -c 200)"
  as "set -e; mkdir -p /tmp/tamperp/src && cd /tmp/tamperp && cp $TGZ ./pkg.tgz && echo '{\"name\":\"p\",\"version\":\"1.0.0\",\"private\":true}' > package.json && pnpm add --offline -D ./pkg.tgz > /dev/null 2>&1
      tar -xzf pkg.tgz -C src && printf 'X' | dd of=src/package/native/nextrs bs=1 seek=4096 conv=notrunc 2> /dev/null
      tar -czf pkg.tgz -C src package && rm -rf node_modules && pnpm install --offline --frozen-lockfile > ci.log 2>&1 && echo exit=0 || echo exit=1" > /tmp/o 2>&1
  rec corrupt:tampered-pnpm "$(! grep -q 'exit=0' /tmp/o && grep -qiE 'integrity|ERR_PNPM' /tmp/tamperp/ci.log; echo $?)" "pnpm install --frozen-lockfile after the tarball changed: $(grep -m1 -iE 'ERR_PNPM|integrity' /tmp/tamperp/ci.log | head -c 200)"
  # A truncated download.
  as "mkdir -p /tmp/trunc && cd /tmp/trunc && head -c 4000000 $TGZ > pkg.tgz && npm init -y > /dev/null && npm install --offline -D ./pkg.tgz > i.log 2>&1; echo exit=\$?" > /tmp/o 2>&1
  rec corrupt:truncated-npm "$(! grep -q 'exit=0' /tmp/o && grep -qiE 'TAR_BAD_ARCHIVE|zlib|unexpected end|Z_BUF_ERROR|EINTEGRITY|TAR_' /tmp/trunc/i.log; echo $?)" "npm install of a truncated tarball: $(grep -m1 -E 'npm error (code|zlib|TAR)|ERR' /tmp/trunc/i.log | head -c 200)"
  # The binary loses its executable bit, or disappears: the launcher says what to do.
  as "mkdir -p /tmp/perm && cd /tmp/perm && npm init -y > /dev/null && npm install --offline -D $TGZ > /dev/null 2>&1 && chmod a-x node_modules/@nextrs-sh/cli/native/nextrs && ./node_modules/.bin/nextrs --version; echo exit=\$?" > /tmp/o 2>&1
  rec broken:not-executable "$(grep -q 'exit=126' /tmp/o && grep -q 'not executable' /tmp/o; echo $?)" "$(grep -m1 'nextrs:' /tmp/o) [$(grep -o 'exit=[0-9]*' /tmp/o)]"
  as "cd /tmp/perm && rm node_modules/@nextrs-sh/cli/native/nextrs && ./node_modules/.bin/nextrs --version; echo exit=\$?" > /tmp/o 2>&1
  rec broken:missing-binary "$(grep -q 'exit=127' /tmp/o && grep -q 'native binary is missing' /tmp/o; echo $?)" "$(grep -m1 'nextrs:' /tmp/o) [$(grep -o 'exit=[0-9]*' /tmp/o)]"
  # Another CPU or OS, as npm and pnpm see it: process.arch / process.platform
  # are overridden in the package manager's own process (simulation; the
  # real musl case runs in an Alpine image, and old glibc on Ubuntu 20.04).
  echo 'Object.defineProperty(process, "arch", { value: "arm64" })' > /tmp/arm64.cjs
  echo 'Object.defineProperty(process, "platform", { value: "darwin" })' > /tmp/darwin.cjs
  chmod a+r /tmp/arm64.cjs /tmp/darwin.cjs
  for sim in arm64 darwin; do
    as "mkdir -p /tmp/plat-$sim && cd /tmp/plat-$sim && npm init -y > /dev/null && NODE_OPTIONS='--require /tmp/$sim.cjs' npm install --offline -D $TGZ; echo exit=\$?" > /tmp/o 2>&1
    rec "unsupported:npm-$sim" "$(! grep -q 'exit=0' /tmp/o && grep -q EBADPLATFORM /tmp/o; echo $?)" "$(grep -m1 -E 'Unsupported platform' /tmp/o | sed 's/npm error notsup //' | head -c 220)"
    as "mkdir -p /tmp/platp-$sim && cd /tmp/platp-$sim && echo '{\"name\":\"p\",\"version\":\"1.0.0\",\"private\":true}' > package.json && NODE_OPTIONS='--require /tmp/$sim.cjs' pnpm add --offline -D $TGZ; echo exit=\$?" > /tmp/o 2>&1
    # pnpm checks os/cpu strictly only for optional dependencies (and with
    # engine-strict); for a direct dependency it installs. Recorded, not a
    # gate: the launcher refuses at run time (see unsupported:launcher-*).
    rec "unsupported:pnpm-$sim" info "pnpm add under simulated $sim: $(grep -o 'exit=[0-9]*' /tmp/o); $(grep -m1 -iE 'ERR_PNPM|unsupported|incompatible|WARN' /tmp/o | head -c 160)"
  done
  # The launcher on another CPU: a uname that reports aarch64.
  mkdir -p /tmp/fakeuname && printf '#!/bin/sh\necho "Linux aarch64"\n' > /tmp/fakeuname/uname && chmod 755 /tmp/fakeuname/uname
  as "mkdir -p /tmp/arch && cd /tmp/arch && npm init -y > /dev/null && npm install --offline -D $TGZ > /dev/null 2>&1; PATH=/tmp/fakeuname:\$PATH ./node_modules/.bin/nextrs --version; echo exit=\$?" > /tmp/o 2>&1
  rec unsupported:launcher-arm64 "$(grep -q 'exit=1$' /tmp/o && grep -q 'Linux x86_64 binary' /tmp/o; echo $?)" "$(grep -m1 'nextrs:' /tmp/o) [$(grep -o 'exit=[0-9]*' /tmp/o)]"
  ;;

missing-lib)
  # node itself needs libgcc_s, so the installed command is run directly (sh only).
  lib=$(ls /lib/x86_64-linux-gnu/libgcc_s.so.1 /usr/lib/x86_64-linux-gnu/libgcc_s.so.1 2> /dev/null | head -1)
  rm -f "$lib"
  out=$(/work/npm-app/node_modules/.bin/nextrs --version 2>&1); rc=$?
  rec missing:libgcc_s "$([[ $rc != 0 ]] && echo "$out" | grep -q 'libgcc_s.so.1'; echo $?)" "removed $lib; exit $rc: $out"
  ;;

old-glibc)
  user
  as "mkdir -p /tmp/old && cd /tmp/old && npm init -y > /dev/null && npm install --offline -D $TGZ" > /tmp/o 2>&1
  rec old-glibc:install $? "npm installs it (glibc's version cannot be declared in package.json): $(tail -1 /tmp/o)"
  as "cd /tmp/old && npx --no-install nextrs --version; echo exit=\$?" > /tmp/o 2>&1
  rec old-glibc:launcher "$(grep -q 'exit=1$' /tmp/o && grep -q 'needs glibc 2.35' /tmp/o; echo $?)" "$(grep -m1 'nextrs:' /tmp/o) [$(grep -o 'exit=[0-9]*' /tmp/o)]"
  as "cd /tmp/old && node_modules/@nextrs-sh/cli/native/nextrs --version; echo exit=\$?" > /tmp/o 2>&1
  rec old-glibc:direct-binary "$(! grep -q 'exit=0' /tmp/o && grep -q 'GLIBC_2.3' /tmp/o; echo $?)" "without the launcher, the loader says: $(grep -m1 GLIBC /tmp/o | head -c 200) [$(grep -o 'exit=[0-9]*' /tmp/o)]"
  ;;

musl)
  user
  as "mkdir -p /tmp/m && cd /tmp/m && npm init -y > /dev/null && npm install --offline -D $TGZ; echo exit=\$?" > /tmp/o 2>&1
  rec musl:npm-refused "$(! grep -q 'exit=0' /tmp/o && grep -q EBADPLATFORM /tmp/o; echo $?)" "$(grep -m1 -E 'EBADPLATFORM|notsup' /tmp/o) $(grep -m1 -iE 'libc' /tmp/o | head -c 160)"
  # pnpm enforces os/cpu/libc only for optional dependencies: it installs the
  # package, and the launcher must refuse at the first run.
  as "mkdir -p /tmp/mp && cd /tmp/mp && echo '{\"name\":\"p\",\"version\":\"1.0.0\",\"private\":true}' > package.json && pnpm add --offline -D $TGZ > add.log 2>&1; echo add-exit=\$?; ./node_modules/.bin/nextrs --version; echo exit=\$?" > /tmp/o 2>&1
  rec musl:pnpm-launcher "$(grep -q 'exit=1$' /tmp/o && grep -q 'musl' /tmp/o; echo $?)" "pnpm add: $(grep -o 'add-exit=[0-9]*' /tmp/o) (installed); first run: $(grep -m1 'nextrs:' /tmp/o) [$(grep -o '^exit=[0-9]*' /tmp/o)]"
  as "cd /tmp/m && npm install --offline --force -D $TGZ > /dev/null 2>&1; ./node_modules/.bin/nextrs --version; echo exit=\$?" > /tmp/o 2>&1
  rec musl:forced-launcher "$(grep -q 'exit=1$' /tmp/o && grep -q 'musl' /tmp/o; echo $?)" "after npm install --force: $(grep -m1 'nextrs:' /tmp/o) [$(grep -o 'exit=[0-9]*' /tmp/o)]"
  ;;
*) echo "unknown mode $MODE" >&2; exit 2 ;;
esac
echo "== $image $MODE: $([[ $fail == 0 ]] && echo PASS || echo FAIL)"
exit $fail
