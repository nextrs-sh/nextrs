# NextRS

Experimental Rust production runtime for official Next.js build output.
The goal is `next build && nextrs start`.

> **Status: Phase 0 (feasibility).** `nextrs start` serves only what the build
> already produced: static assets, prerendered pages, RSC payloads and
> not-found pages. Every route that needs server-side JavaScript gets an
> explicit `501 Not Implemented`. NextRS does **not** run dynamic Next.js
> routes yet. See [the feasibility report](docs/PHASE-0-FEASIBILITY-REPORT.md).

## Layout

```text
crates/nextrs-adapter   reads .next (Deployment Adapter output or internal manifests)
crates/nextrs-server    axum server for prerendered output (no JS execution)
crates/nextrs-cli       `nextrs inspect` / `nextrs start`
packages/next-adapter   Next.js Deployment Adapter writing .next/nextrs/build-output.json
fixtures/next-app       Next.js 16.4.3 compatibility fixture (App Router, 5 routes)
experiments/js-runtime  V8 (deno_core) + QuickJS (rquickjs) execution experiment (own crate)
experiments/route-handler-harness  Node host calling Next route handlers directly
tests/compat            HTTP comparison against `next start`
tests/data              snapshots of real .next output used by `cargo test`
benchmarks              baseline benchmark driver and raw results
docs                    research, ADR, feasibility report
```

## Try it

Requires Rust 1.88+ and Node.js 20.9+ (for `next build`).

```bash
cd fixtures/next-app && npm ci && npx next build && cd ../..
# optional: build through the public Deployment Adapter API instead
#   NEXT_ADAPTER_PATH=$PWD/packages/next-adapter/index.mjs npx next build

cargo run --bin nextrs -- inspect ./fixtures/next-app
cargo run --bin nextrs -- start ./fixtures/next-app --port 3001   # binds 127.0.0.1

# compare against next start
(cd fixtures/next-app && npx next start -H 127.0.0.1 -p 3000 &)
node tests/compat/compare.mjs --baseline http://127.0.0.1:3000 --candidate http://127.0.0.1:3001
```

`nextrs start` refuses to start if the build uses features it would serve
incorrectly, such as middleware, `next.config` headers/redirects/rewrites,
Server Actions, ISR, i18n or `basePath`. `--allow-unsupported` overrides this.

## Checks

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The JS engine experiment builds separately because it links V8:
`experiments/js-runtime/run-all.sh`.

## Do not expose

NextRS is experimental. It binds loopback by default. Do not put it, or the
JS engine experiments, in front of untrusted traffic.
