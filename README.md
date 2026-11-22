<p align="center">
  <img src=".github/assets/nextrs-banner.svg" width="100%" alt="NextRS: Build with Next.js. Run with Rust.">
</p>

<p align="center">
  <strong>An experimental Rust runtime for Next.js build output.</strong><br>
  Build with Next.js as usual. Run production output with NextRS.
</p>

<p align="center">
  <a href="#project-status"><img src="https://img.shields.io/badge/status-experimental-CE422B?style=flat-square" alt="Status: experimental"></a>
  <a href="engineering/compatibility.md"><img src="https://img.shields.io/badge/tested%20with-Next.js%2016.4.6-000000?style=flat-square&logo=nextdotjs&logoColor=white" alt="Tested with Next.js 16.4.6"></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Rust-1.88%2B-CE422B?style=flat-square&logo=rust&logoColor=white" alt="Rust 1.88 or later"></a>
</p>

NextRS serves the `.next` output of an unmodified `next build` from one Rust
process. Static files and prerendered pages are answered from Rust; dynamic
routes run Next.js's own compiled server code in embedded V8. It replaces
`next start`, not `next build`.

```bash
npx next build                            # build with Next.js, as usual
nextrs start . --js-runtime embedded-v8   # serve the build with NextRS
```

<p align="center"><sub>There is no npm package yet, and no release has been published: build <code>nextrs</code> from source with the <a href="#quick-start">Quick Start</a>.</sub></p>

> [!WARNING]
> **NextRS is experimental.** It is tested with Next.js 16.4.x builds on
> Linux x86_64, against test applications and one larger real application,
> and it has not run in production. It is not a drop-in replacement for
> `next start` for every application, and it is not ready for untrusted
> public traffic. Read the [security notice](engineering/security.md) before
> exposing it to a network.

## Why NextRS?

- **Keep your Next.js workflow.** Develop and build with Next.js and Node.js
  as you do today. NextRS does not rewrite React, Next.js or your code.
- **Rust handles the serving layer.** HTTP, routing, `next.config` rules,
  static files, prerendered pages and image optimization run in an
  [axum](https://github.com/tokio-rs/axum) + Tokio server.
- **Next.js's own handlers run in embedded V8.** No Node.js process serves
  the web application. Backend services it calls, such as an API or a
  database, are separate and keep running as they do today.
- **Resources are explicit.** Isolate pools, queues, timeouts, body sizes and
  outbound network access are bounded and set from the command line.

## Features

- **Rust HTTP serving:** static assets, `public/` files, prerendered HTML and
  RSC payloads, with Next's caching headers.
- **App Router** *(experimental):* streaming server rendering and React
  Server Components, Route Handlers, Server Actions, ISR and on-demand
  revalidation, Cache Components and partial prerendering.
- **Pages Router** *(experimental):* `getServerSideProps`,
  `getStaticProps` with ISR, API routes, Preview Mode, `/_next/data` and
  Next's `i18n` routing.
- **Proxy and routing** *(experimental):* `proxy.ts` and middleware on the
  Node.js runtime, `next.config` headers, redirects and rewrites,
  `basePath` and `trailingSlash`, in `next start`'s order.
- **Shared cache** *(experimental):* one data and response cache for every
  isolate, optionally persisted to disk.
- **Image optimization** *(experimental):* `/_next/image` in Rust, without
  `sharp`, within a memory budget.
- **Operations:** outbound network denied by default, supervised isolates,
  health and readiness probes, Prometheus metrics and JSON logs.

Not supported: the edge runtime, interception routes, and Next's `i18n`
config with App Router routes. NextRS refuses to start a build it would
serve incorrectly. See the [compatibility reference](engineering/compatibility.md).

## Performance

Measured against the official Next.js production server using identical
build output. The chart highlights one Dynamic App Router SSR workload from
November 18, 2026. A separate historical 24-workload matrix and its raw
results are available in the [benchmark documentation](engineering/benchmarks.md).

<p align="center">
  <picture>
    <source media="(max-width: 600px)" srcset=".github/assets/nextrs-benchmarks-mobile.svg">
    <img src=".github/assets/nextrs-benchmarks.svg" width="100%" alt="NextRS vs next start, Dynamic App Router SSR (/books/[id]), measured November 18, 2026 on NextRS f5c480d, Next.js 16.4.6 and Node.js 24.18.0, same Next.js build, median of 3 interleaved rounds. NextRS with one isolate against next start: throughput 20% higher (736 against 611 req/s); p95 latency 47% lower (3.30 against 6.26 ms); anonymous memory 63% less (141 against 376 MiB); CPU time per request 9% less (1.75 against 1.92 ms). Scale across V8 isolates: NextRS with 4 isolates reached 1,898 req/s, 3.11 times next start, sharing the same four-CPU server allocation; the Node baseline is one server process.">
  </picture>
</p>

> [!NOTE]
> **Benchmark notice.** Results were measured on November 18, 2026, using a
> pre-release NextRS build (`f5c480d`), Next.js 16.4.6 and Node.js 24.18.0
> on an Intel Core i5-9400F. Throughput, CPU and memory use closed-loop
> measurements; p95 latency uses equal offered load. Results vary across
> workloads. Some Pages Router routes have higher tail latency, and image
> performance depends on concurrency. See the
> [full methodology, historical 24-workload matrix and raw data](engineering/benchmarks.md).

Image performance depends on concurrency: `next start` was faster than
NextRS at its default two image workers, while NextRS exceeded it with four
workers on the tested four-CPU host. Increasing workers may reduce CPU
capacity available for rendering.
([Image capacity results](engineering/benchmarks.md#image-capacity-phase-3d-2026-11-19).)

## Architecture

<p align="center">
  <img src=".github/assets/nextrs-architecture.svg" width="100%" alt="Architecture: a Next.js app is compiled by next build into the .next output. The NextRS runtime, one Rust process, reads it. Its HTTP server applies routing and next.config rules. Static files, prerendered HTML and RSC, and image optimization are served from Rust. Dynamic SSR, Route Handlers, Server Actions and proxy.ts run Next.js's compiled handlers in embedded V8, which use a shared cache held in Rust.">
</p>

NextRS reads the `.next` output at startup and answers every request from one
Rust process. Static and prerendered responses never touch JavaScript.
Dynamic routes, Server Actions and `proxy.ts` run Next.js's own compiled
handlers in a pool of embedded V8 isolates, and their cache entries live in a
Rust store shared by every isolate.
[More in the architecture reference.](engineering/architecture.md)

## Quick Start

NextRS is not published yet: there is no npm package, no GitHub release and
no crates.io crate (the crate named `nextrs` there is an unrelated project).
Build it from source and serve the example app in
[`fixtures/next-app`](fixtures/next-app/).

**You need:** Linux, Rust 1.88 or later, Node.js 20.9 or later (for
`next build` only), and network access for the first build, which downloads
a prebuilt V8 library.

```bash
# 1. Build the NextRS CLI (V8 is linked statically, so this takes a while)
git clone https://github.com/nextrs-sh/nextrs.git
cd nextrs
cargo build --release -p nextrs-cli

# 2. Build the example app with Next.js, as usual
(cd fixtures/next-app && npm ci && npx next build)

# 3. Serve it with NextRS; dynamic routes need the embedded V8 runtime
./target/release/nextrs start fixtures/next-app --port 3001 --js-runtime embedded-v8
```

Open <http://127.0.0.1:3001>, or try it from another terminal:

```bash
curl -i http://127.0.0.1:3001/           # prerendered page, served from Rust
curl -i http://127.0.0.1:3001/books/42   # dynamic page, rendered by Next.js in V8
```

- **Without `--js-runtime embedded-v8`,** NextRS runs no JavaScript: dynamic
  routes answer `501`, and a build that needs JavaScript (Server Actions or
  ISR, like this one) is refused at startup.
- **`./target/release/nextrs inspect fixtures/next-app`** shows how each
  route will be served.
- **`./target/release/nextrs doctor fixtures/next-app`** checks, without
  running anything, whether an application and its build suit NextRS, and
  says what to change ([doctor.md](engineering/doctor.md); in release
  binaries from 0.1.0-rc.3).
- **`./target/release/nextrs start --help`** lists every option; the
  [configuration reference](engineering/configuration.md) explains limits,
  network access, caching and reverse proxies.
- **If the build runs out of memory,** limit parallel jobs with
  `cargo build --release -j 2 -p nextrs-cli`.

**Precompiled binaries** for Linux x86_64 (glibc 2.35 or later) are
attached to [GitHub releases](https://github.com/nextrs-sh/nextrs/releases)
and published on npm as `@nextrs/cli` (dist-tag `next`):

```bash
npm install -D @nextrs/cli@0.1.0-rc.2
```

A precompiled binary needs no Rust toolchain, Cargo or Node.js to run;
Node.js is still needed for `next build`. The
[installation and operations guide](release/README.md) covers it.

## Project Status

NextRS is a research project, developed in the open.

- **Experimental:** not recommended for untrusted production traffic. It
  binds to `127.0.0.1` by default.
- **Tested scope:** Next.js 16.4.6 and 16.4.5 builds on Linux x86_64. Other
  16.x versions start with a warning; other majors are refused.
- **Tested applications:** App Router and Pages Router fixtures compared
  response by response with `next start`, independent sample applications,
  and one larger application.
- **Real-world example:** the unchanged
  [Velasia Airways](https://github.com/nextrs-sh/velasia-airways) web
  frontend (App Router, Cache Components, Server Actions) passed its own
  Playwright suite on NextRS at 1 and 4 isolates on November 18, 2026:
  flight search, booking, checkout and account management, 101 of 111 tests,
  with 10 mobile-only tests skipped by design
  ([results](benchmarks/certification/results/velasia-airways/)). Its
  Fastify API is a separate service and still runs on Node.js.
- **A narrow Node.js layer:** Node's network modules (`http`, `net`, TCP
  sockets), `zlib` streams, `child_process` and native addons are
  unavailable, so many database drivers will not work.
- **Isolation limits:** a V8 isolate is a memory boundary, not a security
  boundary, and CPU limits apply per isolate, not per request.
- **Release:** v0.1.0-rc.2, an experimental prerelease. It adds
  `deploymentId` support, gzip compression with a cache for immutable
  responses, byte ranges and RSC route-handler parity to v0.1.0-rc.1
  ([release notes](release/v0.1.0-rc.2-release-notes.md)).

## Documentation

[Compatibility](engineering/compatibility.md) ·
[Configuration](engineering/configuration.md) ·
[Operations](engineering/operations.md) ·
[Security](engineering/security.md) ·
[Architecture](engineering/architecture.md) ·
[Benchmarks](engineering/benchmarks.md) ·
[Installation guide](release/README.md) ·
[Engineering reference](engineering/README.md)

## Contributing

Issues and pull requests are welcome. Please
[open an issue](https://github.com/nextrs-sh/nextrs/issues) before starting a
larger change. Every change must pass the Rust checks, and changes to serving
behavior should pass the comparison against `next start`; see
[testing](engineering/testing.md).

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE),
at your option, as the workspace manifest ([`Cargo.toml`](Cargo.toml))
declares. The release archives also carry the license and notice texts of
every bundled third-party component (`THIRD-PARTY-LICENSES.txt`), including
V8 and the Deno JavaScript compiled into the binary.

NextRS is an independent project. It is not affiliated with, endorsed by or
supported by Vercel or the Next.js team. "Next.js" is a trademark of
Vercel, Inc.
