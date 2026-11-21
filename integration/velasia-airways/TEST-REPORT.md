# Test report: Velasia Airways on `@nextrs/cli@0.1.0-rc.1` (Docker)

**Date:** 2026-11-20. **Outcome: A — fully operational** for every workflow tested automatically.
The servicing flows (change seats/extras/flight, cancel) and the payment decline, timeout and
pending outcomes were not automated; they are in the manual checklist.

Velasia Airways ran unmodified, served by the native `nextrs` binary from the published npm
package, with no Node.js in the serving container.
- **Smoke tests:** 66/66 passed (33 in Chromium, 33 in Firefox).
- **Runtime checks:** 15/15 passed.
- **Server errors:** NextRS handled about 7,800 requests. It recorded no handler errors, isolate
  restarts, panics, timeouts or rejections. Every 5xx response was a deliberate refusal of a
  cross-site Server Action.

| | |
|---|---|
| NextRS package | `@nextrs/cli@0.1.0-rc.1`, from `https://registry.npmjs.org/@nextrs/cli/-/cli-0.1.0-rc.1.tgz`, integrity `sha512-RnUTCEw2…yj/7w==`, pinned exactly (no `^`/`~`) |
| Binary | `native/nextrs`, sha256 `44f04e586c7dc9eb0b8f69dfd992143f725472aa5f3619df5a4039d77d2c6956` (= `BUILD-INFO.json`), `nextrs 0.1.0-rc.1`, built from `711d77f`, rustc 1.98.0 |
| Platform | linux/amd64 (`x86_64-unknown-linux-gnu`), needs glibc ≥ 2.35; runtime base `debian:12-slim` (glibc 2.36); links only `libc`, `libm`, `libgcc_s`, `ld-linux` |
| Velasia Airways | `main` at `f03de91fcb2303deb38c2b0602f69e2c6ffb1066`, clean |
| Next.js / React | 16.4.6 / 19.3.1; App Router, Cache Components + PPR, Server Actions (45), `output: 'standalone'`, monorepo (pnpm 10.23.0, Turborepo) |
| Images | `nextrs-velasia-web:local` `sha256:3bfa4c41…` (347 MB, user `nextrs`, entrypoint `/usr/local/bin/nextrs`); `nextrs-velasia-api:local` `sha256:7790bdd5…` (425 MB, Velasia's own `docker/Dockerfile.api`) |
| Docker | Docker Desktop 4.99 (engine 29.9.3), Compose v5.7.0, on Fedora (kernel 7.3.4) |
| Browsers | Playwright 1.65.1: Chrome for Testing 159.0.8154.6 (headless shell), Firefox 159.0 |

## How to start it

```bash
docker compose -f integration/velasia-airways/compose.yaml up --build -d
# → http://localhost:3000   (both services "healthy" in `… ps`; ~55 s from images, ~15 min first build here)
integration/velasia-airways/scripts/verify-runtime.sh     # proves NextRS is serving
integration/velasia-airways/scripts/smoke-test.sh          # browser smoke tests
```

The host needs only Docker. Nothing compiles Rust: the NextRS binary comes from the npm package,
and Node.js runs only in the build stage and in Velasia's API container. See
[`README.md`](README.md) for every command, and
[`MANUAL-TEST-CHECKLIST.md`](MANUAL-TEST-CHECKLIST.md) for manual testing.

## What runs

| Service | Process | Role | Exposed |
|---|---|---|---|
| `velasia-web` | `/usr/local/bin/nextrs start /app/apps/web --js-runtime embedded-v8 --isolates 2 …` | Serves every page, RSC payload, route handler, Server Action, static file and optimised image | `127.0.0.1:3000` (host). Ops listener :9100 container-only |
| `velasia-api` | `node dist/server.mjs` (Fastify + TalaDB) | Velasia's retailing API: a declared supporting backend that renders nothing | Internal network only (`172.30.47.0/24`, no internet) |

The NextRS command line also sets these flags:
- **`--js-env`** for each variable Velasia reads: `RETAILING_API_URL`, `SITE_URL`,
  `RETAILING_API_TIMEOUT_MS` and `VELASIA_TODAY`.
- **`--net-allow http://velasia-api:4010 --net-allow-addr 172.30.47.0/24`**, so server code can
  reach the API and nothing else.
- **`--strip-forwarded-headers`**, because no proxy sits in front.
- **Persistent data and image cache directories** on a volume.

The container runs as UID 10001 with a read-only root filesystem, all capabilities dropped and
`no-new-privileges` set.

## Runtime verification (`evidence/runtime-verification.txt`)

All 15 checks passed, re-run after all test traffic:

1. **npm package:** the image records `@nextrs/cli@0.1.0-rc.1`, resolved from the
   registry.npmjs.org tarball. A clean-room `npm ci` in a fresh `node:24` container reproduces
   the version, checksum and libraries (`evidence/npm-install-checks.txt`). The package has no
   install scripts.
2. **Binary checksum:** the binary in the image has sha256 `44f04e58…`, matching `BUILD-INFO`.
   Running `--version` prints `nextrs 0.1.0-rc.1`.
3. **Entrypoint:** the container entrypoint is `/usr/local/bin/nextrs`.
4. **Processes:** `/proc` in `velasia-web` shows only `docker-init` (PID 1) and `nextrs start …`
   (PID 7). There is no `node`, `npm`, `pnpm`, `next start` or `server.js` process. No Node.js
   executable exists in the image, and the build fails if one does.
5. **Port 3000:** it is listening inside `velasia-web`, which is the only container publishing
   it. `velasia-api` publishes no host port.
6. **Request counting:** NextRS's own `nextrs_http_requests_total` counter rose for
   browser-facing probes (`/`, `/destinations`, `/api/airports`, `/flights`, a missing static
   file → 404).
7. **Response headers:** they come from NextRS serving Next's output: `x-nextjs-cache: HIT`,
   `x-nextjs-prerender`, `x-nextjs-postponed` (PPR) and Velasia's `next.config` security headers.
8. **NextRS startup log:** `embedded V8 executor ready isolates=2`,
   `Server Actions enabled actions=45`, `outbound fetch enabled allow=http://velasia-api:4010`
   and `nextrs ready addr=0.0.0.0:3000 build_id=i35V8ZnDo4vkF6as9Slvl ready_ms=160`.

## Build-output compatibility (`evidence/nextrs-inspect.txt`)

`nextrs inspect /app/apps/web` runs in the image build, and the build fails if it fails. It
recognised the following:
- Next.js 16.4.6 and the monorepo standalone layout.
- 45 routes: pages with ISR or prerendered HTML/RSC/segments, 17 route handlers (14 under
  `/api`), and 7 dynamic routes with `generateStaticParams`.
- 1 header rule and 2 redirects from `next.config`.

Its `[unsupported]` diagnostics cover "45 Server Actions" and "ISR regeneration". Both mean
"requires JavaScript" and are served by `--js-runtime embedded-v8`, which the tests exercised.
It also flags the dynamic routes with `generateStaticParams` as "fallback behaviour not
modelled". In practice, request-time params worked for:
- `/destinations/[slug]`;
- `/booking/confirmation/[orderId]`;
- `/manage-booking/[orderId]`.

`nextrs start` did not refuse the build, and `--allow-unsupported` is **not** used.

## Smoke tests (`evidence/smoke-summary.md`)

`smoke/tests/velasia-nextrs.spec.ts` uses only routes and selectors that exist in Velasia
(taken from its `apps/web/src/app` and `tests/e2e`, read-only). Every test fails on any uncaught
page error, console error (hydration errors included) or failed or ≥ 400 request it does not
expect.

| Area | Tests (each in Chromium and Firefox) | Chromium | Firefox |
|---|---|---|---|
| Delivery | Homepage server HTML; CSS/JS delivered with `immutable` caching and applied; hydration without errors; `next/image` optimisation and public images; `robots.txt`, `sitemap.xml`, manifest; security headers; cache-control (static homepage `s-maxage`, personal pages `private, no-store`) | 7 PASS | 7 PASS |
| Routing | Client navigation over 4 sections with RSC (`text/x-component`) payloads and no document reloads; dynamic `/destinations/[slug]` plus refresh; back/forward; unknown page → **404** with the app's not-found page; `next.config` redirects (308, 307); mobile menu at 390 px with no horizontal overflow | 6 PASS | 6 PASS |
| Route handlers | `/api/airports`, `/api/destinations`, `/api/fare-families`, `/api/aircraft`; live `/api/flights` (`private, no-store`); health live/ready; invalid search → 4xx | 4 PASS | 4 PASS |
| Search and results | Homepage search form (airport comboboxes, dates) → `/flights` results; stops filter and nearby dates; invalid search → the app's alert | 3 PASS | 3 PASS |
| Booking journey | Flight and fare → travellers (Server Action) → seat (held by the API) → extras → review; review refresh; checkout (`PENDING_PAYMENT`) → Demo Card payment → confirmation `VEL-XXXXXX`; manage-booking lookup by reference plus refresh; confirmation hard refresh; no browser errors during the journey | 10 PASS | 10 PASS |
| Server Actions | Unknown reference → the app's error; cross-site `Origin` refused (same-site 200) | 2 PASS | 2 PASS |
| Accounts | Register a demo account → sign out (cookies cleared) → sign in → My Trips | 1 PASS | 1 PASS |
| **Total** | | **33/33** | **33/33** |

No test is BLOCKED or NOT TESTED. Before the final run, 4 rounds of failures were fixed. Every
fix was in the test or the environment; NextRS needed none:
- **Two test bugs:** the search test didn't fill in the dates, and the 404 test didn't expect
  the browser's own console line about the 404.
- **Two wrong expectations about Velasia's behaviour:** sign-in lands on `/account/trips`, and
  the sign-in route stays mounted but hidden (React Activity).
- **One Velasia setting:** demo registration is off in production. `compose.yaml` turns it on
  with Velasia's own `ACCOUNT_REGISTRATION=enabled`.

## Working routes

Everything tested above worked. That covers `/`, `/destinations`, `/destinations/<slug>`,
`/flights` (static shell and PPR results), `/booking/travellers`, `/booking/seats`,
`/booking/extras`, `/booking/review`, `/booking/checkout`,
`/booking/confirmation/<orderId>`, `/manage-booking`, `/manage-booking/<orderId>`,
`/account/register`, `/account/sign-in`, `/account/trips`, `/help`, the not-found page, all
`/api/*` handlers listed above, `/robots.txt`, `/sitemap.xml`, `/manifest.webmanifest`,
`/_next/static/*` and `/_next/image`.

**Not exercised automatically:**
- `/manage-booking/<orderId>/{seats,extras,change-flight,cancel}`;
- `/account`, `/account/{travellers,settings}`, `/about` and `/credits`;
- `/api/offers*`, `/api/routes`, `/api/flights/[id]*` and `/api/internal/metrics`. These are
  either called internally by pages or need a token.

## Unsupported routes and integration blockers

**None found.** No route answered 501, and no route depends on a fallback.

## Server-side errors and warnings (`evidence/nextrs-velasia-web.log`, `evidence/nextrs-metrics-excerpt.txt`)

| Observation | Count | Assessment |
|---|---|---|
| `nextrs_http_requests_total` | 2xx 7,805 · 3xx 16 · 4xx 17 · 5xx 8 | Health checks are included in the 2xx count |
| 5xx responses | 8 | All of them are the cross-site Server Action test, 2 browsers × 4 runs. Next.js's own action handler logs "`x-forwarded-host` … does not match `origin` … `evil.example` … Aborting the action" and throws "Invalid Server Actions request". This is the intended CSRF refusal, not a NextRS fault |
| Executor handler errors, restarts, panics, timeouts, rejections; fetch failures; cache errors | 0 | — |
| `WARN nextrs::js: node:crypto.generatePrimeSync / generateKeySync is not implemented (read as undefined)` | 2 each, at isolate start | Missing Node API members, not used by any tested flow. Watch for this if a later flow needs them |
| `retailing api call failed … REGISTRATION_DISABLED` | 2 | From the run before demo registration was enabled (see the Smoke tests section) |
| Velasia API errors | 0 | — |

## Browser errors

- **Chromium:** none.
- **Firefox:** 19 occurrences of one app-level console message. Firefox logs
  "Content-Security-Policy: … blocked a JavaScript eval … (Missing 'unsafe-eval')" when zod v4
  probes for eval support with `try { Function("") } catch {}` in the client chunk
  `3_wgr03mdhsz9.js`. Velasia's CSP intentionally blocks it, the code catches it, and nothing
  breaks. The chunk NextRS serves is byte-identical to the `next build` output (sha256
  `813b6069…` in both), and the CSP header comes from Velasia's `next.config`. So this is
  independent of the server runtime and would appear under `next start` too. The tests count it
  in the summary instead of failing on it; that is the only console message they tolerate.

## Application immutability (`evidence/velasia-baseline-{before,after}.txt`)

Velasia's state was captured before any setup (05:57Z) and again after all builds and test
runs.

| Check | Result |
|---|---|
| Tracked files changed | None (`git diff HEAD` hash identical, empty) |
| Unexpected files in the worktree | None: 3,416 paths, ignored files included (`.next`, `.turbo`, `test-results`), identical list. 0 files anywhere, `node_modules` included, modified since the baseline |
| Commits | None (79 commits, all refs hash identical) |
| Branch | `main` at `f03de91`, unchanged |
| Existing uncommitted changes | None existed; status still empty |
| Worktrees and stash | Unchanged: the 3 worktrees (2 already "prunable") and 0 stashes. Nothing was pruned |

How the source stays untouched:
- Velasia is only a read-only BuildKit context, and its own `.dockerignore` keeps
  `node_modules`, `.next`, `.git`, `.env*` and databases out. The context sent was 11 MB.
- Dependencies, the build and generated files live in image layers only.

**NextRS clone:** `main` at `0b31678`, not compiled. Its existing uncommitted files are
unchanged; the only addition is the untracked `integration/` directory.

## Findings for NextRS (package and documentation; nothing blocks this integration)

1. **Package README checksum instruction is wrong.**
   - It says `bin/nextrs` is "the native NextRS binary" and to verify it with
     `sha256sum node_modules/@nextrs/cli/bin/nextrs`.
   - In 0.1.0-rc.1, `bin/nextrs` is a 1,885-byte `#!/bin/sh` launcher with sha256
     `de785ce9…`, so the documented check always fails.
   - The binary is `native/nextrs` (`44f04e58…`). The `examples/docker` Dockerfile already
     checks the right file.
2. **Stale wording in `nextrs inspect`:** "Node.js APIs: … not provided by NextRS Phase 0".
3. **Duplicate header:** `x-nextjs-prerender: 1` is sent twice on `/` (a prerendered HIT).
   It's harmless, but `next start` sends it once.
4. **`nextrs inspect` diagnostics read as blocking but aren't:**
   - The labels `[unsupported] server-actions` and `[unsupported] isr` appear even though
     `--js-runtime embedded-v8` serves both.
   - "Fallback behaviour is not modelled" is shown for routes that worked.
5. **The repository README on `main` still says "There is no npm package yet".**

## Known limitations of this environment

- **Real date.** "Today" is the real date in Asia/Manila, so inventory, prices and seat maps
  change from day to day. Set `VELASIA_TODAY` and `SMOKE_TODAY` for deterministic runs; this
  needs `--build`.
- **Payment outcomes.** By default every demo payment is approved. Decline, timeout and pending
  need `PAYMENT_OUTCOME_CONTROLS=enabled`.
- **Build-time API wait.** Velasia's build script gives its build-time API 30 s to start. The
  Dockerfile pre-starts the same API with a 180 s wait, using the script's documented "already
  answering `/health`" path. This changes nothing in the build or its output. The pnpm line
  `ERR_PNPM_RECURSIVE_RUN_FIRST_FAIL … tsx src/server.ts` that follows the build is that API
  being stopped; the step's exit status is the build's own.
- **x86_64 only.** The package is linux-x64 glibc only, so on Apple Silicon or arm64 hosts the
  images need x86_64 emulation, which this run did not test.
- **Single run on one machine.** Docker Desktop's VM (6 CPUs, 3.8 GB) was shared with other
  workloads during the first builds, which explains the transient registry timeouts in the
  build log.
- **Not tested:** performance and soak were out of scope. The manual checklist covers the
  servicing flows that were not automated.

## Manual testing

Follow [`MANUAL-TEST-CHECKLIST.md`](MANUAL-TEST-CHECKLIST.md): 42 tests in 14 sections,
written for both Chrome and Firefox. To record a failure, see [`README.md`](README.md),
"When something fails". Do not patch Velasia Airways; record the failure as a NextRS
compatibility gap.
