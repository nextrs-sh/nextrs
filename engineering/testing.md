# Testing

How NextRS is checked against `next start`, how to run the checks, and how
the benchmarks and soaks are measured.

- [Rust checks](#rust-checks)
- [Comparison suites](#comparison-suites)
- [Running a comparison by hand](#running-a-comparison-by-hand)
- [Oracle checks and negative controls](#oracle-checks-and-negative-controls)
- [Continuous integration](#continuous-integration)
- [Benchmarks and soaks](#benchmarks-and-soaks)

## Rust checks

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

`cargo test` links V8 through `nextrs-runtime`. The unit and HTTP tests use
[`tests/data`](../tests/data/), redacted snapshots of real Next.js 16.4.3
`.next` output (default and adapter builds), so they do not need Node.js.
The executor's own tests (backpressure, supervision and fault injection,
deadlines, the isolate pool, request bodies) use small handler fixtures and
need no Next.js build. Outbound fetch is tested against local upstreams
only, never public services.

Tests that need the real fixture build are ignored by default. They cover
dynamic routes, the proxy, and reliability over real sockets: slow and
disconnecting clients, body limits, overload, CPU stalls and shutdown, plus
process memory with paused clients.

```bash
(cd fixtures/next-app && npm ci && npx next build)
cargo test -p nextrs-server --test dynamic --test reliability --test memory --test proxy -- --ignored
```

[`scripts/snapshot-build.mjs`](../scripts/snapshot-build.mjs) regenerates a
snapshot from a `.next` directory and replaces build-time secrets with
placeholders.

## Comparison suites

A response counts as compatible only when it matches what `next start`
returns for the same build. [`scripts/compat-suite.sh`](../scripts/compat-suite.sh)
builds the fixtures (optionally against another Next.js version, in a
temporary copy) and runs the suites against both servers:

```bash
scripts/compat-suite.sh --isolates 1 --out compat-out \
  --browser --cache-components --proxy --proxy-combinations --images
scripts/realworld-suite.sh --isolates 1 --out realworld-out --browser
```

`--next 16.4.5` runs the same suites against another Next.js release.

| Suite | What it checks |
|---|---|
| [`compare.mjs`](../tests/compat/compare.mjs) | Status, headers and body of every fixture case against `next start`. An explicit `501` is reported as UNSUPPORTED, never as a match |
| [`semantic.mjs`](../tests/compat/semantic.mjs) | Server-rendered DOM and the parsed RSC Flight tree, for output that is equivalent but chunked differently |
| [`isolation.mjs`](../tests/compat/isolation.mjs) | Many overlapping requests with unique headers, cookies and params, plus client aborts: no request may see another's data |
| [`streaming.mjs`](../tests/compat/streaming.mjs) | The Suspense shell reaches the client before the delayed boundary content |
| [`features.mjs`](../tests/compat/features.mjs) | Isolation beyond headers: `React.cache`, concurrent Suspense, error boundaries, `after()`, concurrent uploads, and the intended scope of `unstable_cache` |
| [`data.mjs`](../tests/compat/data.mjs) | Server-side `fetch` and the data cache through a local upstream ([`upstream.mjs`](../tests/compat/upstream.mjs)) that counts requests: caching modes, revalidation, tags, cache identity, dedupe, errors, cancellation, sharing across isolates |
| [`restart.mjs`](../tests/compat/restart.mjs) | Cache persistence across a restart of both servers: data entries and an ISR page |
| [`isr.mjs`](../tests/compat/isr.mjs) | Fresh and stale serving, background regeneration, on-demand params, ISR route handlers, 100 concurrent requests on a stale page, failed regeneration, Server Actions that revalidate, visibility in every isolate, private routes and cache poisoning attempts |
| [`cache-components.mjs`](../tests/compat/cache-components.mjs) | `"use cache"`, `cacheLife` and `cacheTag`, sharing across isolates, and an entry across a restart |
| [`actions.mjs`](../tests/compat/actions.mjs) | Server Actions: origin (CSRF) and method checks, invalid and foreign action IDs, encrypted closures (valid and tampered), body limits, hostile payloads, concurrent mutations, disconnects, cache invalidation; NextRS's own limits, isolate failure (no replay) and deadlines |
| [`proxy.mjs`](../tests/compat/proxy.mjs) | `proxy.ts`: continue, redirects, rewrites, header and cookie changes, matchers with `has`/`missing`, direct responses, errors, bodies, protected prerendered and ISR pages, Server Actions through the proxy (checked against an upstream ledger), internal-header and path-normalization bypass attempts; timeout, stall and abort handling |
| [`routing-config.mjs`](../tests/compat/routing-config.mjs) | `next.config` headers, redirects and rewrites in default, `basePath`, `trailingSlash` and combined builds |
| [`config-flags.mjs`](../tests/compat/config-flags.mjs) | `assetPrefix` and `experimental.caseSensitiveRoutes` builds |
| [`auth-proxy.mjs`](../tests/compat/auth-proxy.mjs) | A `jose` session proxy: valid, expired, re-signed, tampered and malformed sessions, protected routes of every kind, bypass attempts, login and logout, concurrent identities |
| [`jwks-proxy.mjs`](../tests/compat/jwks-proxy.mjs) | `jose` with a remote JWKS against a local OpenID provider ([`oidc-stub.mjs`](../tests/compat/oidc-stub.mjs)): five signature algorithms, claim failures, tampering, algorithm confusion, key rotation, provider failures, the network policy |
| [`authjs.mjs`](../tests/compat/authjs.mjs) | Auth.js v5: credentials sign-in with CSRF, the OpenID Connect code flow with PKCE, state and nonce, forged callbacks and ID tokens, sign-out, a protected Server Action, concurrent users, an executor restart |
| [`intl-proxy.mjs`](../tests/compat/intl-proxy.mjs), [`intl-variants.mjs`](../tests/compat/intl-variants.mjs) | `next-intl` locale detection, redirects, prefixed routes, RSC, locale-aware Server Actions, and its other routing modes |
| [`platform-proxy.mjs`](../tests/compat/platform-proxy.mjs) | A proxy that fetches before deciding, external rewrites, `waitUntil`, `instrumentation.ts`, Draft Mode, `dynamicParams = false` and ISR fallback |
| [`proxy-shutdown.mjs`](../tests/compat/proxy-shutdown.mjs) | NextRS only: graceful shutdown with proxy work in flight (Server Actions never run twice) |
| [`otel.mjs`](../tests/compat/otel.mjs) | OpenTelemetry through `@vercel/otel` to a local collector: trace propagation, span nesting across awaits, concurrent traces, export failures |
| [`image.mjs`](../tests/compat/image.mjs) | `/_next/image`: parameter validation, traversal attempts, formats, sizes, orientation, colour profiles, alpha, animated and corrupt sources, decompression bombs, conditional requests, cache behaviour, remote sources through a local upstream ([`image-upstream.mjs`](../tests/compat/image-upstream.mjs)). Images are compared decoded: format, size and alpha exactly, pixels by fidelity to an unencoded reference |
| [`browser/actions.mjs`](../tests/browser/actions.mjs) | Server Actions in Chromium (playwright-core), with and without JavaScript: what a user sees, cookies, every action request's status and headers |
| [`browser/proxy.mjs`](../tests/browser/proxy.mjs) | Client-side navigation through the proxy and config rules |
| [`browser/realworld.mjs`](../tests/browser/realworld.mjs) | The `jose` and `next-intl` applications: login and logout, redirects back, a protected mutation, language detection, locale switching |
| [`browser/image.mjs`](../tests/browser/image.mjs) | `next/image`: selected sources and colours, lazy loading, preload, blur placeholders, responsive selection, broken sources, revalidation |

Each script documents its usage in its header comment. Recorded runs are kept
next to the scripts as JSON (`tests/compat/*.json`).

## Running a comparison by hand

Start the official server on port 3000 and NextRS on port 3001, then run the
scripts from the repository root:

```bash
(cd fixtures/next-app && npx next start -H 127.0.0.1 -p 3000 &)
./target/release/nextrs start ./fixtures/next-app --port 3001 --js-runtime embedded-v8 &

node tests/compat/compare.mjs   --baseline http://127.0.0.1:3000 --candidate http://127.0.0.1:3001
node tests/compat/semantic.mjs  --baseline http://127.0.0.1:3000 --candidate http://127.0.0.1:3001
node tests/compat/isolation.mjs --target http://127.0.0.1:3001
node tests/compat/streaming.mjs --target http://127.0.0.1:3001
```

`semantic.mjs`, `isolation.mjs` and `streaming.mjs` exercise dynamic routes,
so NextRS must run with `--js-runtime embedded-v8`. For proxy comparisons,
start `next start` with `-H 0.0.0.0`: bound to `127.0.0.1` it proxies
rewrites to itself differently.

## Oracle checks and negative controls

Every critical scenario also states its expected outcome, checked on both
servers: when the two servers agree on a wrong answer (a fixture or test
defect), the suite reports `ORACLE-FAIL` instead of a match.

Negative controls (`realworld-suite.sh --negative-controls`, and the
`negative-controls` cargo feature for the image engine) rerun the suites with
deliberate defects (a repeated mutation, an action that never runs,
credentials dropped, another user's data, lost cookies, wrong redirects,
broken image limits) and must fail.

## Continuous integration

[`.github/workflows/ci.yml`](../.github/workflows/ci.yml) runs on pushes to
`main` and on pull requests:

- `cargo fmt`, `clippy -D warnings` and `cargo test` (Rust 1.98.0)
- `cargo audit` against the RustSec database
- The fixture-dependent ignored tests
- `compat-suite.sh` and `realworld-suite.sh` on the pinned Next.js 16.4.3 with
  1 and 4 isolates (gating), and on 16.4.4 and 16.4.5 (reported, not gating)
- Graceful shutdown with proxy work in flight

## Benchmarks and soaks

Benchmark results, their method and limitations, and the benchmark and soak
drivers are documented in [benchmarks.md](benchmarks.md).
