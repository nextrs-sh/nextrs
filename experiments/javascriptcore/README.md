# JavaScriptCore spike (Phase 1A, Task J)

A bounded experiment, not a runtime backend. It embeds the system's
WebKitGTK JavaScriptCore through its public C API
(`<JavaScriptCore/JavaScript.h>`). It runs the **same engine-agnostic
drivers** as the Phase 0 V8/QuickJS experiment (`../js-runtime/js`), so the
results are directly comparable.

Requirements: `javascriptcoregtk-4.1` with headers and pkg-config file
(Fedora: `javascriptcoregtk4.1-devel`). The crate is standalone (its own
`[workspace]`), so the main workspace never links JSC.

```bash
cd experiments/javascriptcore
cargo build --release
./target/release/jsc-experiment hello
./target/release/jsc-experiment async
./target/release/jsc-experiment react      # React 19.3.1 streaming SSR (Phase 0 bundle)
./target/release/jsc-experiment rsc        # RSC Flight (Phase 0 bundle)
./target/release/jsc-experiment probe      # async-context and V8-only API probes
./target/release/jsc-experiment next ../../fixtures/next-app .next/server/app/dynamic/page.js 2
```

The host provides only what the Phase 0 hosts provided: `__report`,
`console`, a Rust timer loop for `setTimeout`, and read-only file
helpers for the `next` test. JSC drains its own microtask queue when the
outermost API call returns. `unsafe` code is confined to `src/jsc.rs`
(FFI).

Results from the reference run are in `results/runs.jsonl`.
