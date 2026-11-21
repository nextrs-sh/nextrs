# Velasia Airways on NextRS (npm release candidate)

Runs the unmodified [Velasia Airways](../../../velasia-airways) storefront through the
**published** NextRS runtime, `@nextrs/cli@0.1.0-rc.1` from registry.npmjs.org, in Docker, for
manual browser testing at <http://localhost:3000>.

- Nothing here compiles NextRS: the image takes the prebuilt binary from the npm package.
  The host needs only Docker (and Node.js only to run the optional smoke tests).
- Velasia Airways is a **read-only** build input. Nothing is written into its checkout.
- The storefront image contains **no Node.js**: only `nextrs` can serve the pages.
  Velasia's own retailing API (Fastify + TalaDB) runs beside it on Node.js, as a declared
  supporting backend that renders nothing and has no host port.

Results of the last run: [`TEST-REPORT.md`](TEST-REPORT.md). Manual tests:
[`MANUAL-TEST-CHECKLIST.md`](MANUAL-TEST-CHECKLIST.md).

## Layout

| Path | Purpose |
|---|---|
| `package.json`, `package-lock.json` | Pins `@nextrs/cli` to exactly `0.1.0-rc.1` (registry tarball and integrity in the lockfile). The image runs `npm ci` on them. |
| `Dockerfile` | A: Velasia's own build (pnpm, `next build`, standalone). B: `npm ci` of the NextRS package, with checks. C: `nextrs inspect` of the build. D: runtime on `debian:12-slim`, no Node.js. |
| `compose.yaml` | `velasia-api` (Velasia's `docker/Dockerfile.api`, unmodified) and `velasia-web` (NextRS). Only `velasia-web` publishes a port. |
| `.env.example` | Optional overrides (copy to `.env`). |
| `scripts/build.sh` | Builds both images. |
| `scripts/verify-runtime.sh` | Proves the server on :3000 is the npm package's `nextrs` binary. |
| `scripts/check-npm-package.sh` | Clean-room `npm ci` of the lockfile in a fresh `node:24` container: version, registry source, checksum, linked libraries. |
| `scripts/smoke-test.sh` | Runtime verification, then Playwright smoke tests (Chromium and Firefox). |
| `smoke/` | The Playwright tests, their own `package.json`/lockfile (`@playwright/test` 1.65.1). |
| `results/` | Generated logs and reports (git-ignored). |
| `evidence/` | The copies of the results that `TEST-REPORT.md` cites. |

The Velasia checkout is expected at `../../../velasia-airways` relative to this directory (the
sibling of the `nextrs` clone). Set `VELASIA_SRC` in `.env` or the environment otherwise.

## Commands

Run from the root of the `nextrs` clone.

```bash
# Start (builds on first run; the first build takes several minutes)
docker compose -f integration/velasia-airways/compose.yaml up --build -d

# Follow logs (both services, or only NextRS)
docker compose -f integration/velasia-airways/compose.yaml logs -f
docker compose -f integration/velasia-airways/compose.yaml logs -f velasia-web

# Status (both should be "healthy")
docker compose -f integration/velasia-airways/compose.yaml ps

# Stop (keeps the orders database and caches)
docker compose -f integration/velasia-airways/compose.yaml down
# Stop and delete the orders database and caches
docker compose -f integration/velasia-airways/compose.yaml down -v
```

If port 3000 is already in use, `up` fails with `Bind for 127.0.0.1:3000 failed: port is
already allocated` (or `address already in use`). Find the owner with `ss -ltnp 'sport = :3000'`
and stop it yourself; nothing here stops other processes.

### Verify the installed NextRS version

```bash
docker compose -f integration/velasia-airways/compose.yaml exec velasia-web nextrs --version
docker compose -f integration/velasia-airways/compose.yaml exec velasia-web cat /usr/share/doc/nextrs/BUILD-INFO.json
docker compose -f integration/velasia-airways/compose.yaml exec velasia-web sha256sum /usr/local/bin/nextrs
# must equal binary_sha256: 44f04e586c7dc9eb0b8f69dfd992143f725472aa5f3619df5a4039d77d2c6956
grep -A3 '"node_modules/@nextrs/cli"' integration/velasia-airways/package-lock.json
```

### Inspect the running executable

```bash
integration/velasia-airways/scripts/verify-runtime.sh    # all checks; writes results/runtime-verification.txt
docker top "$(docker compose -f integration/velasia-airways/compose.yaml ps -q velasia-web)"
docker compose -f integration/velasia-airways/compose.yaml exec velasia-web \
  bash -c 'for p in /proc/[0-9]*; do echo "$(readlink $p/exe) :: $(tr "\0" " " < $p/cmdline)"; done'
```

### Run the smoke tests

Needs Node.js 20 or later on the host (it only drives the browsers).

```bash
(cd integration/velasia-airways/smoke && npm ci && npx playwright install chromium firefox)
integration/velasia-airways/scripts/smoke-test.sh                    # both browsers
SMOKE_PROJECTS=chromium integration/velasia-airways/scripts/smoke-test.sh
integration/velasia-airways/scripts/smoke-test.sh -g 'booking journey' # one group
```

Results: `results/smoke/summary.md` (PASS / FAIL / BLOCKED / NOT TESTED per test) and
`results/smoke/html/index.html` (traces and screenshots of failures).

### Inspect HTTP responses

```bash
curl -sS -D - -o /dev/null http://localhost:3000/
curl -sS -D - -o /dev/null 'http://localhost:3000/api/airports'
curl -sS -D - -o /dev/null -H 'RSC: 1' http://localhost:3000/destinations   # RSC (Flight) payload
curl -sS -D - -o /dev/null --max-redirs 0 http://localhost:3000/checkout     # redirect from next.config
# NextRS's operations listener (not published): readiness, liveness, Prometheus metrics
docker compose -f integration/velasia-airways/compose.yaml exec velasia-web bash -c \
  'exec 3<>/dev/tcp/127.0.0.1/9100; printf "GET /metrics HTTP/1.0\r\n\r\n" >&3; cat <&3' | grep '^nextrs_' | head -40
```

### Identify unsupported Next.js features

`nextrs inspect` runs during the image build against Velasia's standalone output; its report is
kept in the image:

```bash
docker run --rm --entrypoint cat nextrs-velasia-web:local /usr/share/doc/nextrs/velasia-inspect.txt
docker run --rm nextrs-velasia-web:local inspect /app/apps/web      # run it again
```

`nextrs start` refuses to start when the build uses a feature NextRS would serve incorrectly
(`--allow-unsupported` overrides that; this harness does **not** pass it). At run time, a route
NextRS cannot execute answers **501**; look for these:

```bash
docker compose -f integration/velasia-airways/compose.yaml logs velasia-web | grep -Ei ' 50[0-9] |501|unsupported|error|panic'
```

More detail per request: `RUST_LOG=nextrs=debug` in `.env`, then `up -d`.

### Rebuild after configuration changes

| You changed | Run |
|---|---|
| `.env` runtime values (`NEXTRS_ISOLATES`, `RUST_LOG`, `PAYMENT_OUTCOME_CONTROLS`, …) | `docker compose -f integration/velasia-airways/compose.yaml up -d` |
| `SITE_URL`, `VELASIA_TODAY`, the Velasia checkout, `Dockerfile`, `package-lock.json` | `docker compose -f integration/velasia-airways/compose.yaml up --build -d` |
| Suspect a stale cache layer | `integration/velasia-airways/scripts/build.sh --no-cache && docker compose -f integration/velasia-airways/compose.yaml up -d` |
| Want a clean orders database | `docker compose -f integration/velasia-airways/compose.yaml down -v && … up -d` |

To move to another NextRS version: `npm install -D --save-exact @nextrs/cli@<version>` in this
directory, update the version and checksum constants in `Dockerfile` and
`scripts/verify-runtime.sh`, rebuild.

## How the runtime is configured

`compose.yaml` starts:

```
/usr/local/bin/nextrs start /app/apps/web --host 0.0.0.0 --port 3000 --js-runtime embedded-v8
  --isolates 2
  --js-env RETAILING_API_URL --js-env SITE_URL --js-env RETAILING_API_TIMEOUT_MS --js-env VELASIA_TODAY
  --net-allow http://velasia-api:4010 --net-allow-addr 172.30.47.0/24
  --strip-forwarded-headers
  --cache-dir /var/cache/nextrs/data --image-cache-dir /var/cache/nextrs/images
  --metrics-addr 0.0.0.0:9100 --log-format text
```

- `/app/apps/web` is Velasia's `.next/standalone/apps/web` (monorepo layout; its build already
  copies `public/` and `.next/static/` beside it).
- **`--js-env`**: NextRS gives JavaScript no host environment by default. These are the
  variables Velasia's `apps/web/src/lib/env.ts` reads (`NODE_ENV` is always `production`).
- **`--net-allow` / `--net-allow-addr`**: NextRS refuses fetches to private addresses unless
  allowed. Server code reaches only the retailing API, on the pinned internal subnet.
- No reverse proxy is in front, so forwarded headers from browsers are dropped.
- The container runs as an unprivileged user with a read-only root file system; only the
  `nextrs-cache` volume and `/tmp` are writable.

## When something fails

Capture, for the report:

1. Route and method (DevTools Network, or the failing smoke test's trace).
2. Status: `curl -sS -D - -o /dev/null -X <METHOD> http://localhost:3000<route>`.
3. NextRS logs around it: `docker compose -f integration/velasia-airways/compose.yaml logs --since 5m velasia-web`.
4. API logs if data is involved: `… logs --since 5m velasia-api`.
5. The suspected feature (Server Action, PPR resume, route handler, image optimisation,
   redirects, …) and whether `nextrs inspect` flagged it.

Do not patch Velasia Airways to work around a failure: record it as a NextRS compatibility gap.
