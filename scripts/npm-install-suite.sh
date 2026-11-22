#!/usr/bin/env bash
# Developer-installation test of the @nextrs/cli tarball in one project.
#
#   scripts/npm-install-suite.sh --pm npm|pnpm --project DIR --tarball FILE.tgz \
#     --expect-sha256 SHA --probe tests/npm/probe.mjs [--port 6300] [--json OUT]
#
# DIR is a Next.js project whose own dependencies are already installed
# (npm ci / pnpm install) and that has not been built. The run should have
# no network access: installing the tarball, `next build` and serving must
# all work offline. Needs bash, coreutils, node and the package manager;
# pnpm is `pnpm` on PATH (or $PNPM).
#
# Checks (pass/fail, JSON lines in --json):
#   install            the tarball installs as a devDependency, offline
#   no-lifecycle       the installed package declares no scripts, and the
#                      install log shows no script, compiler or download
#   native-binary      the installed native/nextrs is the release binary
#                      (SHA-256), executable
#   bin-link           how node_modules/.bin/nextrs reaches it (npm symlink,
#                      pnpm shim that execs)
#   version, help, inspect   through `npx`/`pnpm exec`
#   doctor-text, doctor-json, doctor-no-node, doctor-invocation-error
#                      `nextrs doctor` through `npx`/`pnpm exec` and the native
#                      binary without Node.js (0.1.0-rc.3 and later)
#   next-build         the official `next build`, with the package installed
#   serve-pm:*         `npx nextrs start` / `pnpm exec nextrs start`: ready,
#                      SSR, RSC, Server Action, static file (tests/npm/probe.mjs)
#   pm-process         the process serving requests is the native binary
#   pm-ctrl-c          Ctrl-C (SIGINT to the process group, as a terminal sends
#                      it) stops the server gracefully
#   pm-sigterm         (info) SIGTERM to the npx/pnpm process alone: whether it
#                      reaches the server. npm exec runs the command through
#                      `sh -c`; where /bin/sh is dash (Debian, Ubuntu), the shell
#                      stays as the server's parent and dies on SIGTERM without
#                      passing it on, so the server is orphaned. Recorded, not a
#                      gate: production must not start the server through npx.
#   serve-bin:*        node_modules/.bin/nextrs start (the production form)
#   bin-process        it is the launched process itself (exec), no parent
#                      shell or Node.js
#   bin-sigint, bin-sigterm   graceful stop, exit 0
#   exit-codes         usage error (2) and a missing app propagate unchanged
#                      through .bin and npx/pnpm exec
set -uo pipefail
PM="" PROJECT="" TARBALL="" EXPECT="" PROBE="" PORT=6300 JSON=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --pm) PM="$2"; shift 2 ;;
    --project) PROJECT="$2"; shift 2 ;;
    --tarball) TARBALL="$2"; shift 2 ;;
    --expect-sha256) EXPECT="$2"; shift 2 ;;
    --probe) PROBE="$2"; shift 2 ;;
    --port) PORT="$2"; shift 2 ;;
    --json) JSON="$2"; shift 2 ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
[[ ($PM == npm || $PM == pnpm) && -d $PROJECT && -f $TARBALL && -n $EXPECT && -f $PROBE ]] ||
  { echo "usage: $0 --pm npm|pnpm --project DIR --tarball FILE.tgz --expect-sha256 SHA --probe probe.mjs [--port P] [--json OUT]" >&2; exit 2; }
set -m # job control: background servers get SIGINT like a foreground command would
PNPM=${PNPM:-pnpm}
METRICS=$((PORT + 1))
URL="http://127.0.0.1:$PORT"
LOGS=$(mktemp -d "${TMPDIR:-/tmp}/nextrs-npm-suite.XXXXXX")
PID=""
cleanup() { [[ -n $PID ]] && kill -9 "$PID" 2>/dev/null; for p in $(servers); do kill -9 "$p" 2>/dev/null; done; }
trap cleanup EXIT
declare -a RESULTS=()
FAILED=0
check() { # name, status (0 = pass, "info" = recorded, not a gate), detail
  local s=pass
  if [[ $2 == info ]]; then s=info; elif [[ $2 != 0 ]]; then s=fail; FAILED=1; fi
  RESULTS+=("$(node -e 'console.log(JSON.stringify({check:process.argv[1],result:process.argv[2],detail:process.argv[3].slice(0,600)}))' "$PM:$1" "$s" "$3")")
  printf '%-30s %s  %s\n' "$PM:$1" "$s" "$(printf '%s' "$3" | head -c 170 | tr '\n' ' ')"
}
# PIDs of native nextrs processes serving this suite's port.
servers() { for p in /proc/[0-9]*; do [[ $(readlink "$p/exe" 2>/dev/null) == */native/nextrs ]] && tr '\0' ' ' < "$p/cmdline" 2>/dev/null | grep -q -- "--port $PORT" && echo "${p#/proc/}"; done; }
ready() { node -e 'fetch(process.argv[1],{signal:AbortSignal.timeout(2000)}).then(r=>process.exit(r.status===200?0:1),()=>process.exit(1))' "http://127.0.0.1:$METRICS/readyz"; }
# Process chain from a PID up to this shell, by executable name.
chain() { local p=$1 out=""; while [[ -n $p && $p != "$$" && $p -gt 1 ]]; do out+="$(basename "$(readlink "/proc/$p/exe" 2>/dev/null)")($p) <- "; p=$(sed 's/.*) //' "/proc/$p/stat" 2>/dev/null | cut -d' ' -f2); done; echo "${out}suite"; }
if [[ $PM == npm ]]; then RUN=(npx --no-install); else RUN=("$PNPM" exec); fi

cd "$PROJECT" || exit 1

# ------------------------------------------------------------------ install
if [[ $PM == npm ]]; then
  npm install --save-dev --offline --no-audit --no-fund --foreground-scripts "$TARBALL" > "$LOGS/install.log" 2>&1
else
  "$PNPM" add -D --offline "$TARBALL" > "$LOGS/install.log" 2>&1
fi
check install $? "$(tail -3 "$LOGS/install.log" | tr '\n' ' ')"
pj=node_modules/@nextrs/cli/package.json
scripts=$(node -e 'const p=JSON.parse(require("fs").readFileSync(process.argv[1]));console.log(Object.keys({...p.scripts}).join(" "))' "$pj" 2>&1)
noisy=$(grep -iE 'node-gyp|gyp info|prebuild|cargo|rustc|postinstall|preinstall|> .*install$|downloading|fetching binary' "$LOGS/install.log" | head -3)
check no-lifecycle "$([[ -z $scripts && -z $noisy ]]; echo $?)" "package scripts: ${scripts:-none}; install log: ${noisy:-no script, compiler or download lines}"

native=$(readlink -f node_modules/@nextrs/cli/native/nextrs)
sha=$(sha256sum "$native" 2>/dev/null | cut -d' ' -f1)
check native-binary "$([[ $sha == "$EXPECT" && -x $native ]]; echo $?)" "$native sha256 ${sha:-missing}, mode $(stat -c %a "$native" 2>/dev/null)"
link=node_modules/.bin/nextrs
if [[ -L $link ]]; then
  how="symlink -> $(readlink "$link")"
  [[ $(readlink -f "$link") == "$(readlink -f node_modules/@nextrs/cli/bin/nextrs)" ]]; ok=$?
else
  how="shim: $(grep -E '^ *exec ' "$link" | head -1 | sed 's/^ *//' | cut -c1-120)"
  grep -qE '^ *exec .*/bin/nextrs" "\$@"' "$link"; ok=$?
fi
check bin-link $ok "$how"

out=$("${RUN[@]}" nextrs --version 2> "$LOGS/version.err"); check version "$([[ $out == "nextrs $(node -p "require('./$pj').version")" ]]; echo $?)" "${RUN[*]} nextrs --version: $out"
"${RUN[@]}" nextrs --help > /dev/null 2>&1 && "${RUN[@]}" nextrs start --help > /dev/null 2>&1; check help $? "nextrs --help, nextrs start --help"

# --------------------------------------------------------------- next build
if [[ $PM == npm ]]; then npx --no-install next build > "$LOGS/build.log" 2>&1; else "$PNPM" exec next build > "$LOGS/build.log" 2>&1; fi
rc=$?
check next-build "$([[ $rc == 0 && -f .next/BUILD_ID ]]; echo $?)" "next build exit $rc, Next.js $(node -p "require('next/package.json').version"), BUILD_ID $(cat .next/BUILD_ID 2>/dev/null)"
out=$("${RUN[@]}" nextrs inspect . 2>&1); check inspect $? "$(echo "$out" | head -3 | tr '\n' ' ')"
# Doctor (0.1.0-rc.3): text and JSON through the package manager, with
# consistent exit codes; the native binary alone with no Node.js on PATH (it
# only reads the existing build); a missing path is an invocation error (2).
out=$("${RUN[@]}" nextrs doctor . 2>&1); rc=$?
check doctor-text "$([[ ($rc == 0 || $rc == 1) && $out == *"NextRS Doctor"* ]]; echo $?)" "${RUN[*]} nextrs doctor . exit $rc: $(echo "$out" | grep -m1 '^Result:')"
"${RUN[@]}" nextrs doctor . --json > "$LOGS/doctor.json" 2> "$LOGS/doctor.err"; jrc=$?
summary=$(node -e 'const r=JSON.parse(require("fs").readFileSync(process.argv[1],"utf8")); console.log(`${r.exitCode} ${r.status} errors=${r.summary.errors} warnings=${r.summary.warnings}`)' "$LOGS/doctor.json" 2>/dev/null)
check doctor-json "$([[ -n $summary && ${summary%% *} == "$jrc" && $jrc == "$rc" ]]; echo $?)" "--json exit $jrc: exitCode/status $summary"
nonode=$(env -i PATH=/usr/bin:/bin HOME="${HOME:-/tmp}" sh -c 'command -v node > /dev/null && echo "node-on-path "; "$0" doctor . --json > /dev/null 2>&1; echo $?' "$native")
check doctor-no-node "$([[ $nonode == "$jrc" ]]; echo $?)" "native/nextrs doctor . --json, no Node.js on PATH: exit $nonode (through ${RUN[0]}: $jrc)"
./node_modules/.bin/nextrs doctor /nonexistent-app > /dev/null 2>&1; a=$?
"${RUN[@]}" nextrs doctor /nonexistent-app > /dev/null 2>&1; b=$?
check doctor-invocation-error "$([[ $a == 2 && $b == 2 ]]; echo $?)" "nextrs doctor /nonexistent-app: .bin $a, ${RUN[0]} $b"

# serve LABEL CMD...: starts the server in the background, waits for readiness.
serve() {
  local label=$1; shift
  "$@" start . --port "$PORT" --js-runtime embedded-v8 --metrics-addr "127.0.0.1:$METRICS" > "$LOGS/$label.log" 2>&1 &
  PID=$!
  local ok=1
  for _ in $(seq 1 120); do ready && { ok=0; break; }; kill -0 "$PID" 2>/dev/null || break; sleep 0.5; done
  check "$label:ready" $ok "$(tail -c 200 "$LOGS/$label.log" | tr '\n' ' ')"
  return $ok
}
probe() { # LABEL [--only list]
  local label=$1; shift
  node "$PROBE" --url "$URL" --project "$PROJECT" "$@" > "$LOGS/$label.probe" 2>&1
  while read -r line; do
    name=$(node -e 'const j=JSON.parse(process.argv[1]);console.log(j.check)' "$line" 2>/dev/null) || { check "$label:probe" 1 "$line"; continue; }
    res=$(node -e 'const j=JSON.parse(process.argv[1]);console.log(j.result+"\t"+j.detail)' "$line")
    check "$label:$name" "$([[ ${res%%$'\t'*} == pass ]]; echo $?)" "${res#*$'\t'}"
  done < "$LOGS/$label.probe"
}
# stop SIGNAL LABEL: signal the launched process, wait, check graceful stop
# and no leftovers. Sets RC (the launched process's exit status) and OK (0
# when the server logged a graceful stop and nothing is left running).
stop() {
  local sig=$1 label=$2 left
  kill "-$sig" "$PID" 2> /dev/null
  for _ in $(seq 1 60); do kill -0 "$PID" 2>/dev/null || break; sleep 0.25; done
  kill -0 "$PID" 2>/dev/null && kill -9 "$PID"
  wait "$PID"; RC=$?; PID=""
  sleep 1
  left=$(servers | tr '\n' ' ')
  for p in $left; do kill -9 "$p" 2>/dev/null; done
  [[ -z $left ]] && grep -q 'nextrs stopped' "$LOGS/$label.log"; OK=$?
}

# ------------------------------------------- through the package manager
if serve serve-pm "${RUN[@]}" nextrs; then
  probe serve-pm
  s=$(servers | head -1)
  check pm-process "$([[ -n $s && $(sha256sum "/proc/$s/exe" | cut -d' ' -f1) == "$EXPECT" ]]; echo $?)" "serving process: $(chain "$s")"
  # Ctrl-C in a terminal: SIGINT to the whole foreground process group.
  kill -INT -- "-$PID"
  for _ in $(seq 1 60); do kill -0 "$PID" 2>/dev/null || break; sleep 0.25; done
  stop KILL serve-pm 2>/dev/null
  check pm-ctrl-c "$OK" "SIGINT to the process group of ${RUN[0]}: graceful stop $([[ $OK == 0 ]] && echo yes || echo NO), nothing left running; ${RUN[0]} exit $RC"
fi
if serve serve-pm2 "${RUN[@]}" nextrs; then
  chain_pm=$(chain "$(servers | head -1)")
  stop TERM serve-pm2
  check pm-sigterm info "SIGTERM to the ${RUN[0]} process only: reached the server and stopped it gracefully: $([[ $OK == 0 ]] && echo yes || echo "NO (the server was orphaned and is killed by this test)"); ${RUN[0]} exit $RC; chain was: $chain_pm; /bin/sh is $(basename "$(readlink -f /bin/sh)")"
fi

# -------------------------------------------- node_modules/.bin/nextrs
if serve serve-bin ./node_modules/.bin/nextrs; then
  probe serve-bin --only ssr,rsc
  s=$(servers | head -1)
  check bin-process "$([[ $s == "$PID" && $(sha256sum "/proc/$s/exe" | cut -d' ' -f1) == "$EXPECT" ]]; echo $?)" "launched PID $PID; serving process: $(chain "$s")"
  stop INT serve-bin
  check bin-sigint "$([[ $OK == 0 && $RC == 0 ]]; echo $?)" "SIGINT: exit $RC, graceful stop $([[ $OK == 0 ]] && echo yes || echo NO)"
fi
if serve serve-bin2 ./node_modules/.bin/nextrs; then
  stop TERM serve-bin2
  check bin-sigterm "$([[ $OK == 0 && $RC == 0 ]]; echo $?)" "SIGTERM: exit $RC, graceful stop $([[ $OK == 0 ]] && echo yes || echo NO)"
fi

# ------------------------------------------------------------ exit codes
./node_modules/.bin/nextrs no-such-command > /dev/null 2>&1; a=$?
"${RUN[@]}" nextrs no-such-command > /dev/null 2>&1; b=$?
./node_modules/.bin/nextrs start /nonexistent-app --port "$PORT" > /dev/null 2>&1; c=$?
"${RUN[@]}" nextrs start /nonexistent-app --port "$PORT" > /dev/null 2>&1; d=$?
check exit-codes "$([[ $a == 2 && $b == 2 && $c != 0 && $c == "$d" ]]; echo $?)" "usage error: .bin $a, ${RUN[0]} $b; missing app: .bin $c, ${RUN[0]} $d"

if [[ -n $JSON ]]; then
  { echo "{\"pm\":\"$PM\",\"pm_version\":\"$("${PM/pnpm/$PNPM}" --version 2>/dev/null)\",\"node\":\"$(node --version)\",\"host\":\"$(. /etc/os-release 2>/dev/null; echo "${PRETTY_NAME:-unknown}")\",\"glibc\":\"$(getconf GNU_LIBC_VERSION 2>/dev/null)\",\"passed\":$([[ $FAILED == 0 ]] && echo true || echo false),\"checks\":["; IFS=,; echo "${RESULTS[*]}"; echo "]}"; } > "$JSON"
fi
[[ $FAILED == 0 ]] || { for f in "$LOGS"/*.log; do echo "--- $f"; tail -15 "$f"; done; }
rm -rf "$LOGS"
exit $FAILED
