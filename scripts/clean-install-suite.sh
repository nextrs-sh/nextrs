#!/usr/bin/env bash
# Feature checks of a NextRS release archive on a clean machine, with curl
# only. Complements release-smoke.sh (install, version, readiness, TLS,
# shutdown) with one request per runtime feature, against the prebuilt
# applications of a bundle from scripts/clean-install-bundle.sh:
#
#   scripts/clean-install-suite.sh --archive FILE.tar.gz --bundle DIR [--isolates N] [--port P] [--json OUT]
#
# Every server runs with an empty environment (env -i, PATH=/usr/bin:/bin)
# from the bundle's application directory, which may be read-only. Each
# check records pass/fail; any failure makes the exit status non-zero.
#
#   next-app               dynamic SSR, RSC payload, streamed route body (Web
#                          Streams), Server Action (headers and cookies of
#                          the calling request), static chunk, two
#                          concurrent identities
#   next-cache-components  PPR: prerendered shell plus a dynamic hole
#                          streamed into it; a "use cache" page
#   next-pages             getServerSideProps, /_next/data JSON, API route
#                          (body parsing), static page
#   next-image             /_next/image: WebP and AVIF from a JPEG, a public
#                          file
#   next-authjs            Auth.js credentials sign-in (CSRF HMAC, JWE
#                          session cookie through Web Crypto), the session
#                          on a protected page, and its absence without it
#
# Each server is stopped with SIGTERM and must exit 0.
set -uo pipefail
ARCHIVE="" BUNDLE="" ISOLATES=1 PORT=3970 JSON=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --archive) ARCHIVE="$2"; shift 2 ;;
    --bundle) BUNDLE="$2"; shift 2 ;;
    --isolates) ISOLATES="$2"; shift 2 ;;
    --port) PORT="$2"; shift 2 ;;
    --json) JSON="$2"; shift 2 ;;
    *) echo "unknown argument $1" >&2; exit 2 ;;
  esac
done
[[ -f $ARCHIVE && -d $BUNDLE/apps && -f $BUNDLE/probes.env ]] || { echo "usage: $0 --archive FILE.tar.gz --bundle DIR [--isolates N]" >&2; exit 2; }
# shellcheck disable=SC1091
. "$BUNDLE/probes.env"
METRICS=$((PORT + 1))
WORK=$(mktemp -d "${TMPDIR:-/tmp}/nextrs-suite.XXXXXX")
PID=""
cleanup() { [[ -n $PID ]] && kill -9 "$PID" 2>/dev/null; rm -rf "$WORK"; }
trap cleanup EXIT
declare -a RESULTS=()
FAILED=0
check() { # name, status (0 = pass), detail
  local s=pass; [[ $2 == 0 ]] || { s=fail; FAILED=1; }
  RESULTS+=("{\"check\":\"$1\",\"result\":\"$s\",\"detail\":$(printf '%s' "$3" | head -c 400 | sed 's/\\/\\\\/g; s/"/\\"/g; s/\t/ /g' | awk 'BEGIN{printf "\""} {printf "%s\\n", $0} END{printf "\""}')}")
  printf '%-30s %s  %s\n' "$1" "$s" "$(printf '%s' "$3" | head -c 140 | tr '\n' ' ')"
}
# req NAME [curl args...]: body in $WORK/NAME.body, headers in $WORK/NAME.head; prints the status.
req() { local n="$1"; shift; curl -s -o "$WORK/$n.body" -D "$WORK/$n.head" -w '%{http_code}' --max-time 30 "$@" || true; }
hdr() { grep -i "^$2:" "$WORK/$1.head" | tail -1 | cut -d' ' -f2- | tr -d '\r'; }

tar -xzf "$ARCHIVE" -C "$WORK"
NEXTRS=$(echo "$WORK"/nextrs-v*/nextrs)
[[ -x $NEXTRS ]] || { echo "no nextrs in $ARCHIVE" >&2; exit 1; }
URL="http://127.0.0.1:$PORT"

# serve FIXTURE [nextrs flags...]
serve() {
  local app="$BUNDLE/apps/$1"; shift
  env -i PATH=/usr/bin:/bin HOME="$WORK/home" NO_COLOR=1 "$@" "$NEXTRS" start "$app" --port "$PORT" \
    --js-runtime embedded-v8 --isolates "$ISOLATES" --metrics-addr "127.0.0.1:$METRICS" \
    --cache-dir "$WORK/cache-$app_name" --image-cache-dir "$WORK/images-$app_name" $SERVE_FLAGS > "$WORK/server-$app_name.log" 2>&1 &
  PID=$!
  local ok=1
  for _ in $(seq 1 120); do [[ $(curl -s -o /dev/null -w '%{http_code}' "http://127.0.0.1:$METRICS/readyz") == 200 ]] && { ok=0; break; }; kill -0 "$PID" 2>/dev/null || break; sleep 0.5; done
  check "$app_name:ready" $ok "$ISOLATES isolate(s); $(tail -c 300 "$WORK/server-$app_name.log" | tr '\n' ' ')"
  return $ok
}
stop() {
  kill -TERM "$PID"; wait "$PID"; local code=$?; PID=""
  check "$app_name:shutdown" "$([[ $code == 0 ]] && grep -q 'nextrs stopped' "$WORK/server-$app_name.log"; echo $?)" "SIGTERM -> exit $code"
  local errors
  errors=$(grep -E '"level":"(ERROR|error)"| ERROR |panicked|\[error\]|Failed to initialize' "$WORK/server-$app_name.log" | grep -v 'MissingCSRF' | head -3)
  check "$app_name:no-errors-logged" "$([[ -z $errors ]]; echo $?)" "${errors:-no ERROR lines, no panic, no application error}"
}

# ------------------------------------------------------------------ App Router
app_name=next-app SERVE_FLAGS=""
if serve next-app; then
  s=$(req ssr "$URL/books/42")
  check app:dynamic-ssr "$([[ $s == 200 ]] && grep -q '42' "$WORK/ssr.body" && grep -qi '^content-type: text/html' "$WORK/ssr.head"; echo $?)" "GET /books/42 -> $s"
  s=$(req rsc -L -H 'RSC: 1' "$URL/books/42")
  check app:rsc-payload "$([[ $s == 200 ]] && [[ $(hdr rsc content-type) == text/x-component* ]] && grep -q '^0:' "$WORK/rsc.body"; echo $?)" "GET /books/42 RSC: 1 -> $s $(hdr rsc content-type), $(wc -c < "$WORK/rsc.body") bytes"
  s=$(req stream "$URL/api/stream?size=1048576")
  check app:streamed-body "$([[ $s == 200 ]] && [[ $(wc -c < "$WORK/stream.body") == 1048576 ]]; echo $?)" "GET /api/stream 1 MiB ReadableStream -> $s, $(wc -c < "$WORK/stream.body") bytes"
  s=$(req action -X POST -H "Next-Action: $APP_ACTION_ID" -H 'Accept: text/x-component' -H 'Content-Type: text/plain;charset=UTF-8' \
    -H "Origin: $URL" -H 'x-test-user: alice' -H 'Cookie: nextrs-flavor=mint' --data '["probe-tag",0]' "$URL$APP_ACTION_PAGE")
  check app:server-action "$([[ $s == 200 ]] && grep -q '"tag":"probe-tag"' "$WORK/action.body" && grep -q '"user":"alice"' "$WORK/action.body" && grep -q '"cookie":"mint"' "$WORK/action.body"; echo $?)" "POST $APP_ACTION_PAGE whoAmI -> $s $(grep -o '"tag":[^}]*' "$WORK/action.body" | head -1)"
  # Two identities at once, each with a delay inside the action: neither
  # may see the other's request headers.
  pids=()
  for u in alice bob; do
    curl -s --max-time 30 -X POST -H "Next-Action: $APP_ACTION_ID" -H 'Accept: text/x-component' -H 'Content-Type: text/plain;charset=UTF-8' \
      -H "Origin: $URL" -H "x-test-user: $u" --data "[\"iso-$u\",300]" "$URL$APP_ACTION_PAGE" > "$WORK/iso-$u.body" &
    pids+=($!)
  done
  wait "${pids[@]}"
  ok=0
  for u in alice bob; do grep -q "\"tag\":\"iso-$u\",\"user\":\"$u\",\"userAfterDelay\":\"$u\"" "$WORK/iso-$u.body" || ok=1; done
  check app:concurrent-identities $ok "alice: $(grep -o '"tag":"iso[^}]*' "$WORK/iso-alice.body" | head -1); bob: $(grep -o '"tag":"iso[^}]*' "$WORK/iso-bob.body" | head -1)"
  s=$(req chunk "$URL$APP_STATIC_CHUNK")
  check app:static-file "$([[ $s == 200 ]] && [[ $(wc -c < "$WORK/chunk.body") == "$APP_STATIC_CHUNK_BYTES" ]] && hdr chunk cache-control | grep -q immutable; echo $?)" "GET $APP_STATIC_CHUNK -> $s, $(wc -c < "$WORK/chunk.body") bytes, cache-control: $(hdr chunk cache-control)"
  stop
fi

# --------------------------------------------------------- Cache Components/PPR
app_name=next-cache-components SERVE_FLAGS=""
if serve next-cache-components; then
  s=$(req ppr "$URL/profile?key=probe")
  check ppr:shell-and-hole "$([[ $s == 200 ]] && grep -q '<main' "$WORK/ppr.body" && grep -q 'key=probe hit=' "$WORK/ppr.body"; echo $?)" "GET /profile?key=probe -> $s; dynamic hole: $(grep -o 'key=probe hit=[a-z]*' "$WORK/ppr.body" | head -1)"
  s=$(req cached "$URL/cached")
  check ppr:use-cache-page "$([[ $s == 200 ]] && grep -qi '^content-type: text/html' "$WORK/cached.head"; echo $?)" "GET /cached -> $s"
  stop
fi

# ---------------------------------------------------------------- Pages Router
app_name=next-pages SERVE_FLAGS=""
if serve next-pages; then
  s=$(req gssp -H 'Cookie: user=alice' "$URL/ssr/whoami")
  check pages:getServerSideProps "$([[ $s == 200 ]] && grep -q 'secret-of-alice' "$WORK/gssp.body"; echo $?)" "GET /ssr/whoami (user=alice) -> $s"
  build=$(cat "$BUNDLE/apps/next-pages/.next/BUILD_ID")
  s=$(req data -H 'Cookie: user=bob' "$URL/_next/data/$build/ssr/whoami.json")
  check pages:next-data-json "$([[ $s == 200 ]] && grep -q '"secret":"secret-of-bob"' "$WORK/data.body"; echo $?)" "GET /_next/data/<build>/ssr/whoami.json (user=bob) -> $s"
  s=$(req api -X POST -H 'Content-Type: application/json' --data '{"probe":1}' "$URL/api/echo?q=1")
  check pages:api-route "$([[ $s == 200 ]] && grep -q '"method":"POST"' "$WORK/api.body" && grep -q '"probe":1' "$WORK/api.body"; echo $?)" "POST /api/echo -> $s $(head -c 120 "$WORK/api.body")"
  s=$(req about "$URL/about")
  check pages:static-page "$([[ $s == 200 ]]; echo $?)" "GET /about -> $s"
  stop
fi

# -------------------------------------------------------- Image optimization
app_name=next-image SERVE_FLAGS=""
if serve next-image; then
  s=$(req webp -H 'Accept: image/webp,*/*' "$URL/_next/image?url=%2Fphotos%2Fphoto.jpg&w=64&q=75")
  magic=$(head -c 12 "$WORK/webp.body" | tail -c 4)
  check image:webp "$([[ $s == 200 && $(hdr webp content-type) == image/webp && $magic == WEBP ]]; echo $?)" "photo.jpg w=64 -> $s $(hdr webp content-type), $(wc -c < "$WORK/webp.body") bytes"
  s=$(req avif -H 'Accept: image/avif,image/webp,*/*' "$URL/_next/image?url=%2Fphotos%2Fphoto.jpg&w=64&q=75")
  check image:avif "$([[ $s == 200 && $(hdr avif content-type) == image/avif ]] && head -c 12 "$WORK/avif.body" | grep -q ftyp; echo $?)" "photo.jpg w=64 -> $s $(hdr avif content-type), $(wc -c < "$WORK/avif.body") bytes"
  s=$(req bad "$URL/_next/image?url=%2Fphotos%2Fphoto.jpg&w=65&q=75")
  check image:rejects-unlisted-width "$([[ $s == 400 ]]; echo $?)" "w=65 -> $s"
  s=$(req public "$URL/photos/small.png")
  check image:public-file "$([[ $s == 200 ]] && [[ $(head -c 4 "$WORK/public.body" | tail -c 3) == PNG ]]; echo $?)" "GET /photos/small.png -> $s"
  stop
fi

# ------------------------------------------------------ Auth.js and Web Crypto
# The fixture also declares an OpenID Connect provider, which Auth.js
# validates on every request; its issuer is never contacted here (only the
# credentials provider signs in), so a closed local port stands in.
app_name=next-authjs SERVE_FLAGS="--js-env AUTH_SECRET --js-env OIDC_ISSUER --js-env OIDC_CLIENT_ID --js-env OIDC_CLIENT_SECRET"
secret=$(head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n')
if serve next-authjs AUTH_SECRET="$secret" OIDC_ISSUER=http://127.0.0.1:9 OIDC_CLIENT_ID=clean-install OIDC_CLIENT_SECRET="${secret:0:24}"; then
  jar="$WORK/jar"
  s=$(req csrf -c "$jar" "$URL/api/auth/csrf")
  token=$(sed -n 's/.*"csrfToken":"\([0-9a-f]*\)".*/\1/p' "$WORK/csrf.body")
  check auth:csrf-token "$([[ $s == 200 && ${#token} == 64 ]]; echo $?)" "GET /api/auth/csrf -> $s, token length ${#token}"
  s=$(req signin -b "$jar" -c "$jar" -X POST --data-urlencode "csrfToken=$token" --data-urlencode username=alice \
    --data-urlencode password=alice-test-password --data-urlencode callbackUrl=/dashboard "$URL/api/auth/callback/credentials")
  check auth:credentials-sign-in "$([[ $s == 302 ]] && grep -q 'session-token' "$jar"; echo $?)" "POST callback/credentials -> $s, location $(hdr signin location)"
  s=$(req dash -b "$jar" "$URL/dashboard")
  check auth:session-page "$([[ $s == 200 ]] && grep -q 'dashboard of <!-- -->Alice\|dashboard of Alice' "$WORK/dash.body"; echo $?)" "GET /dashboard (session) -> $s"
  s=$(req session -b "$jar" "$URL/api/auth/session")
  check auth:session-json "$([[ $s == 200 ]] && grep -q 'alice@example.test' "$WORK/session.body"; echo $?)" "GET /api/auth/session -> $s"
  s=$(req anon "$URL/dashboard")
  check auth:no-session-denied "$([[ $s == 30[27] ]] && ! grep -q 'dashboard of' "$WORK/anon.body"; echo $?)" "GET /dashboard (no cookie) -> $s $(hdr anon location)"
  s=$(req forged -b "$jar" -X POST --data-urlencode "csrfToken=$(printf '%064d' 0)" --data-urlencode username=alice \
    --data-urlencode password=alice-test-password "$URL/api/auth/callback/credentials")
  # Auth.js logs "[auth][error] MissingCSRF" for this request, as it should.
  check auth:forged-csrf-rejected "$([[ $s != 200 ]] && ! hdr forged location | grep -q '/dashboard$'; echo $?)" "forged token -> $s $(hdr forged location)"
  stop
fi

if [[ -n $JSON ]]; then
  { echo "{\"archive\":\"$(basename "$ARCHIVE")\",\"isolates\":$ISOLATES,\"host\":\"$(. /etc/os-release 2>/dev/null; echo "${PRETTY_NAME:-unknown}")\",\"user\":\"$(id -un)\",\"date\":\"$(date -u +%FT%TZ)\",\"passed\":$([[ $FAILED == 0 ]] && echo true || echo false),\"checks\":["; IFS=,; echo "${RESULTS[*]}"; echo "]}"; } > "$JSON"
fi
exit $FAILED
