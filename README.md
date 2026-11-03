# NextRS

Experimental Rust production runtime for official Next.js build output.
The goal is `next build && nextrs start`.

> **Status: Phase 1A (experimental).** `nextrs start` serves static assets,
> prerendered pages, RSC payloads and not-found pages from Rust. With
> `--js-runtime embedded-v8` it also renders dynamic App Router routes by
> running Next's own compiled route handlers in an embedded V8 isolate. There
> is no Node.js or Deno process. Without that flag, dynamic routes get an
> explicit `501 Not Implemented`. Server Actions, middleware, ISR,
> `next/image` and the Pages Router are not implemented.

## Layout

```text
crates/nextrs-adapter   reads .next (Deployment Adapter output or internal manifests)
crates/nextrs-server    axum server: prerendered output, dynamic routes via the executor
crates/nextrs-runtime   embedded V8 executor (deno_core + deno_web + NextRS Node layer)
crates/nextrs-cli       `nextrs inspect` / `nextrs start`
packages/next-adapter   Next.js Deployment Adapter writing .next/nextrs/build-output.json
fixtures/next-app       Next.js 16.4.3 compatibility fixture (App Router, 5 routes)
experiments/js-runtime  Phase 0 V8 (deno_core) + QuickJS (rquickjs) experiment (own crate)
experiments/javascriptcore  Phase 1A JavaScriptCore spike (own crate, system JSC)
experiments/route-handler-harness  Node host calling Next route handlers directly
tests/compat            byte, semantic, isolation and streaming checks against `next start`
tests/data              snapshots of real .next output used by `cargo test`
benchmarks              baseline benchmark driver and raw results
```

## Try it

Requires Rust 1.88+ and Node.js 20.9+ (for `next build`).

```bash
cd fixtures/next-app && npm ci && npx next build && cd ../..
# optional: build through the public Deployment Adapter API instead
#   NEXT_ADAPTER_PATH=$PWD/packages/next-adapter/index.mjs npx next build

cargo run --bin nextrs -- inspect ./fixtures/next-app
cargo run --release --bin nextrs -- start ./fixtures/next-app --port 3001 \
    --js-runtime embedded-v8          # binds 127.0.0.1; omit the flag for static-only

# compare against next start
(cd fixtures/next-app && npx next start -H 127.0.0.1 -p 3000 &)
node tests/compat/compare.mjs   --baseline http://127.0.0.1:3000 --candidate http://127.0.0.1:3001
node tests/compat/semantic.mjs  --baseline http://127.0.0.1:3000 --candidate http://127.0.0.1:3001
node tests/compat/isolation.mjs --target http://127.0.0.1:3001
node tests/compat/streaming.mjs --target http://127.0.0.1:3001
```

`nextrs start` refuses to start if the build uses features it would serve
incorrectly, such as middleware, `next.config` headers/redirects/rewrites,
Server Actions, ISR, i18n or `basePath`. `--allow-unsupported` overrides this.

JavaScript run by the executor can read only `.next` and `node_modules`, cannot
write files, open network connections or spawn processes, and sees only
`NODE_ENV=production` plus the variables passed with `--js-env NAME`.

## Checks

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

`cargo test` links V8 (`nextrs-runtime`). Tests that need the real fixture
build are ignored by default:
`cargo test -p nextrs-server --test dynamic -- --ignored`.

The engine experiments build separately: `experiments/js-runtime/run-all.sh`
and `experiments/javascriptcore` (see its README).

## Do not expose

NextRS is experimental. It binds loopback by default. Do not put it, or the
JS engine experiments, in front of untrusted traffic. A V8 isolate is a memory
boundary, not a security boundary: run one application per process.
