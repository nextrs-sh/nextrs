<p align="center">
  <img src=".github/assets/nextrs-banner.svg" width="100%" alt="NextRS: Build with Next.js. Run with Rust.">
</p>

<p align="center">
  <b>An experimental production runtime for Next.js.</b><br>
  It serves the output of an official <code>next build</code> from a Rust
  HTTP server, and runs the compiled server code in embedded V8.
</p>

<p align="center">
  <a href="#project-status"><img src="https://img.shields.io/badge/status-experimental-CE422B?style=flat-square" alt="Status: experimental"></a>
  <a href="https://www.npmjs.com/package/@nextrs/cli"><img src="https://img.shields.io/npm/v/%40nextrs%2Fcli/next?style=flat-square&label=npm%20%40next&color=E8762D" alt="npm: @nextrs/cli, dist-tag next"></a>
  <a href="engineering/compatibility.md"><img src="https://img.shields.io/badge/tested%20with-Next.js%2016.4.6-000000?style=flat-square&logo=nextdotjs&logoColor=white" alt="Tested with Next.js 16.4.6"></a>
  <a href="#license"><img src="https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-30363D?style=flat-square" alt="License: MIT or Apache-2.0"></a>
</p>

<p align="center">
  <a href="#quick-start"><b>Quick start</b></a> ·
  <a href="engineering/compatibility.md"><b>Compatibility</b></a> ·
  <a href="#documentation"><b>Docs</b></a> ·
  <a href="https://github.com/nextrs-sh/nextrs/releases"><b>Releases</b></a> ·
  <a href="#roadmap"><b>Roadmap</b></a>
</p>

---

You develop and build your application with Next.js, exactly as today.
Where you would run `next start`, you run `nextrs start`. Static files and
prerendered pages are answered by Rust. Dynamic routes run Next.js's own
compiled handlers inside embedded V8, so no separate Node.js server process
serves the application.

NextRS is a research project. It explores whether Next.js production
serving can become more predictable and efficient by giving the HTTP
layer, caching and resource limits to Rust, while staying compatible with
the framework's own build output.

> [!WARNING]
> **Experimental.** NextRS is tested with Next.js 16.4.x on Linux x86_64 and
> has not run in production. It does not support every Next.js feature or
> every Node.js package. Do not expose it to untrusted traffic before
> reading the [security notice](engineering/security.md).

## What is NextRS?

A Next.js application reaches production in two steps: `next build`
compiles it, and a server, normally `next start`, answers requests with
that output. NextRS is an alternative for the second step only.

| | Next.js today | With NextRS |
|---|---|---|
| **Your code** | Next.js and React | Unchanged |
| **Develop** | `next dev` | `next dev` |
| **Build** | `next build` (Node.js) | `next build` (Node.js) |
| **Serve in production** | `next start` (Node.js) | `nextrs start` (Rust + embedded V8) |
| **Your backends** | APIs, databases, auth | Unchanged, separate processes |

It is written in Rust, with [axum](https://github.com/tokio-rs/axum) and
Tokio for HTTP, and embeds the V8 JavaScript engine (through `deno_core`)
to run the parts of Next.js that must execute JavaScript. It reads the same
`.next` directory `next start` reads, and treats `next start` as the
reference: a response counts as compatible only when it matches what the
official server returns for the same build.

## How it works

<p align="center">
  <picture>
    <source media="(max-width: 600px)" srcset=".github/assets/nextrs-architecture-mobile.svg">
    <img src=".github/assets/nextrs-architecture.svg" width="100%" alt="Architecture: a Next.js app (App Router, Pages Router) is compiled by next build into the .next output. The NextRS runtime, one Rust process, reads it. Its HTTP server applies routing and next.config rules. Static files, prerendered HTML and RSC, and image optimization are served from Rust. Dynamic SSR, Route Handlers, Server Actions and proxy.ts run Next.js's compiled handlers in embedded V8, which use a shared cache held in Rust. Your backends (APIs, databases, auth) stay separate processes, reached with fetch only where --net-allow permits.">
  </picture>
</p>

1. **Build with Next.js.** `next build` produces `.next` as usual. Node.js is
   needed here, and only here.
2. **Rust answers what needs no JavaScript.** Static assets, `public/`
   files, prerendered HTML and RSC payloads, `next.config` routing rules and
   `/_next/image` are served directly from Rust.
3. **V8 runs what does.** Dynamic rendering, Route Handlers, Server Actions,
   ISR regeneration and `proxy.ts` run Next.js's compiled handlers in a pool
   of V8 isolates (`--js-runtime embedded-v8`). Their cache entries live in
   one Rust store shared by every isolate.
4. **Your backends stay where they are.** Server code reaches them with
   `fetch`, which is denied unless `--net-allow` names the destination.

A V8 isolate is a memory boundary, not a security sandbox, and CPU limits
apply per isolate, not per request. The
[architecture reference](engineering/architecture.md) covers the request
lifecycle in depth.

## Why NextRS?

- **The same Next.js workflow.** No new framework, no rewrite and no
  adapter code in your application. You build with Next.js and keep
  `next dev`.
- **A Rust serving layer.** HTTP, routing, static and prerendered responses,
  compression and image optimization run in a Rust server, without
  JavaScript on those paths.
- **Supervised JavaScript execution.** Next.js's handlers run in a pool of
  embedded V8 isolates with bounded admission, deadlines, a CPU stall
  watchdog and automatic restart.
- **Explicit resource controls.** Concurrency, queues, timeouts, request
  bodies, heap size, caches and outbound network access are each bounded
  and set from the command line. Outbound network access is off by default.
- **Measured, not assumed.** CI compares NextRS with `next start` response
  by response on every pull request, and performance is measured against
  it on published workloads, including the ones where NextRS is slower
  ([Performance](#performance)).

## Quick start

**Requirements:** Linux x86_64 with glibc 2.35 or later (Ubuntu 22.04+,
Debian 12+), a Next.js 16.4.x application, and Node.js for `next build`.
The npm package carries a precompiled binary: running NextRS needs no Rust,
Cargo or Node.js.

In your Next.js application:

```bash
# 1. Install the current release candidate (prereleases use exact versions)
npm install --save-dev @nextrs/cli@0.1.0-rc.2

# 2. Build with Next.js, as usual
npx next build

# 3. See how NextRS will serve each route, and anything it does not support
npx nextrs inspect .

# 4. Serve the build; dynamic routes need the embedded V8 runtime
npx nextrs start . --js-runtime embedded-v8
```

NextRS listens on <http://127.0.0.1:3000> by default (`--port`, `--host`).

- **`nextrs doctor`**, a read-only compatibility check of your project,
  build and planned options, arrives in **0.1.0-rc.3** (merged, not yet
  published). It is not in rc.2. See [Doctor](engineering/doctor.md).
- **pnpm** works the same way: `pnpm add -D @nextrs/cli@<version>`, then
  `pnpm exec nextrs …`.
- **In production,** start `./node_modules/.bin/nextrs` directly, not
  through `npx` or `npm start`, so that SIGTERM reaches the server.
- **Standalone binary, Docker and pnpm details:** see the
  [installation guide](engineering/installation.md), which also covers
  [building from source](engineering/installation.md#build-from-source).
  Every option is in
  `nextrs start --help` and the
  [configuration reference](engineering/configuration.md).

## Features

Statuses follow the [compatibility matrix](engineering/compatibility.md#capability-matrix).
**Experimental** means it passes the comparison suites against `next start`
on the test applications, with the gaps the matrix lists.

| Area | What NextRS does | Status |
|---|---|---|
| **Static serving** | `/_next/static`, `public/`, prerendered HTML, RSC payloads and the not-found page, with Next.js's caching headers | Supported |
| **Build inspection** | `nextrs inspect` reports how each route is served; unsupported features are refused at startup rather than served wrongly | Supported |
| **App Router rendering** | Dynamic SSR with streaming, React Server Components, Route Handlers, `generateStaticParams`, Draft Mode | Experimental |
| **Server Actions** | Forms, client calls, uploads, redirects and cookies, through Next.js's own handlers | Experimental |
| **Caching and ISR** | The Next.js data cache, ISR and on-demand revalidation in one cache shared by every isolate, optionally persisted (`--cache-dir`) | Experimental |
| **Cache Components** | `"use cache"`, `cacheLife`, `cacheTag`, Partial Prerendering shells | Partial |
| **Pages Router** | `getServerSideProps`, `getStaticProps` with ISR, API routes, Preview Mode, `/_next/data`, Next.js `i18n` routing | Experimental |
| **Proxy and routing** | `proxy.ts` / middleware on the Node.js runtime; `next.config` headers, redirects and rewrites in `next start`'s order; `basePath`, `trailingSlash` | Experimental |
| **Images** | `/_next/image` in Rust, without `sharp`, within a memory budget | Experimental |
| **HTTP** | gzip compression with a cache, byte ranges, conditional requests, `deploymentId` | Experimental |
| **Operations** | Multi-isolate pool with supervision, outbound network policy, health and readiness probes, Prometheus metrics, JSON logs | Experimental |
| **Diagnostics** | `nextrs doctor`: read-only checks of the project, build, Node.js API use and runtime options | In 0.1.0-rc.3 (unreleased) |

**Not supported:** the edge runtime, interception routes, Next.js's `i18n`
config with App Router routes, and WebSockets through external rewrites.
Node.js support is a narrow layer: `http`, `net`, TCP sockets,
`child_process`, `worker_threads` and native addons are unavailable, so
many database drivers do not work in server code. The
[known gaps](engineering/compatibility.md#known-gaps) list every
difference from `next start`.

## Performance

NextRS is benchmarked against `next start` on the same Next.js build. The
chart shows one workload, dynamic App Router SSR (`/books/[id]`), measured
on November 18, 2026.

<p align="center">
  <picture>
    <source media="(max-width: 600px)" srcset=".github/assets/nextrs-benchmarks-mobile.svg">
    <img src=".github/assets/nextrs-benchmarks.svg" width="100%" alt="NextRS vs next start, Dynamic App Router SSR (/books/[id]), measured November 18, 2026 on NextRS f5c480d, Next.js 16.4.6 and Node.js 24.18.0, same Next.js build, median of 3 interleaved rounds. NextRS with one isolate against next start: throughput 20% higher (736 against 611 req/s); p95 latency 47% lower (3.30 against 6.26 ms); anonymous memory 63% less (141 against 376 MiB); CPU time per request 9% less (1.75 against 1.92 ms). Scale across V8 isolates: NextRS with 4 isolates reached 1,898 req/s, 3.11 times next start, sharing the same four-CPU server allocation; the Node baseline is one server process.">
  </picture>
</p>

**What it shows.** With one isolate (one JavaScript thread, as `next start`
has), NextRS served this route at 736 req/s against 611, with a p95 of
3.30 ms against 6.26 ms, 141 MiB of anonymous memory against 376 MiB, and
1.75 ms of CPU per request against 1.92 ms. Four isolates reached
1,898 req/s on the same four CPUs; that is extra parallel JavaScript, not
an equal-worker comparison, and it used more memory (509 MiB).

**What it does not show.** The results come from one Intel Core i5-9400F,
a pre-release build (`f5c480d`) and requests without compression. In the
wider [24-workload matrix](engineering/benchmarks.md#full-matrix-phase-3a-2026-11-16),
NextRS is not ahead everywhere: at its default two image workers it
transforms fewer images per second than `next start` (more with four),
one Pages Router route has a higher p95 at low load, and four isolates
scale 2.1–3.6×, not 4×. Measure your own application before drawing
conclusions. [Method, environment and raw data →](engineering/benchmarks.md)

## Project status

| | |
|---|---|
| **Stage** | Experimental, developed in the open. Not production-ready; binds to `127.0.0.1` by default. |
| **Published release** | [v0.1.0-rc.2](https://github.com/nextrs-sh/nextrs/releases), a prerelease, on GitHub Releases and on npm as `@nextrs/cli` (dist-tag `next`). |
| **Platform** | Linux x86_64, glibc 2.35+. macOS, Windows, arm64 and musl are refused. |
| **Next.js** | 16.4.6 (CI baseline) and 16.4.5, React 19.3.1. Other 16.x start with a warning; other majors are refused. |
| **Tested on** | Test applications compared response by response with `next start`, independent sample applications, and the unchanged [Velasia Airways](https://github.com/nextrs-sh/velasia-airways) storefront: 101 of 111 browser tests passed at 1 and 4 isolates on November 18, 2026, with 10 mobile-only tests skipped by design ([results](benchmarks/certification/results/velasia-airways/)). |
| **Not yet done** | Production use, cloud certification, per-request CPU limits. See [known gaps](engineering/compatibility.md#known-gaps) and [security](engineering/security.md). |

## Roadmap

NextRS is under active development. The roadmap focuses on expanding
Next.js compatibility, improving developer experience, optimizing runtime
performance, and making production behavior increasingly reliable. These
are directions, not commitments: there are no dates, and priorities follow
what the comparison suites and measurements show.

| Theme | Direction |
|---|---|
| **Developer experience** | `nextrs doctor` is implemented, ships in 0.1.0-rc.3, and will keep growing. **Planned:** `nextrs dev`, a development workflow built around the official Next.js tooling; `nextrs preview`, to run and check a production build locally; clearer CLI errors, simpler configuration, more examples. |
| **Next.js compatibility** | Broader App Router and Pages Router coverage, more Node.js APIs and third-party packages, authentication and database integrations, the features refused today (interception routes, `i18n` with the App Router), an investigation of the edge runtime, and new Next.js versions as they ship. Full compatibility is the aim, not a promise. |
| **Performance and efficiency** | Throughput, latency, CPU and memory under load; isolate scheduling and multi-core use; startup time; static and image serving; allocation and memory retention, including bounding concurrent compression-cache fills, which current measurements motivate. |
| **Reliability and security** | Per-request execution limits, predictable resource bounds, cache correctness, request isolation, graceful shutdown and recovery, network and dependency security, observability. |
| **Testing and certification** | More and larger real-world applications, regression testing against `next start`, browser end-to-end runs, load, soak and fault-injection tests, cloud deployment validation, and performance regression tracking. |
| **Deployment and platforms** | Docker and deployment recipes, CI/CD integration, cloud platform guides, broader Linux coverage, and other operating systems and architectures where feasible. |

Completed phases and candidate milestones are in the
[engineering roadmap](engineering/roadmap.md).

## Documentation

| I want to… | Read |
|---|---|
| Install NextRS and run my app | [Installation](engineering/installation.md) · [Doctor](engineering/doctor.md) |
| Know whether my app will work | [Compatibility](engineering/compatibility.md) |
| Tune limits, caching, networking and images | [Configuration](engineering/configuration.md) |
| Deploy and operate it | [Operations](engineering/operations.md) · [Security](engineering/security.md) · [Release guide](release/README.md) |
| Understand the design | [Architecture](engineering/architecture.md) · [Distribution](engineering/distribution.md) |
| Check the numbers | [Benchmarks](engineering/benchmarks.md) · [Testing](engineering/testing.md) |
| See everything | [Engineering reference](engineering/README.md) |

## Contributing

Contributions are welcome, and some of the most useful ones need no Rust:

- **Try NextRS on your application** and
  [open an issue](https://github.com/nextrs-sh/nextrs/issues) with what
  worked and what did not. Include the output of `nextrs inspect` (and, from
  0.1.0-rc.3, `nextrs doctor --json`).
- **Report a compatibility difference** from `next start`, ideally with a
  small reproduction.
- **Improve the documentation** where it confused you.

The code is five Rust crates under [`crates/`](crates/): the build reader,
the HTTP server, the V8 runtime, the image engine and the CLI
([repository structure](engineering/architecture.md#repository-structure)).
For code changes, please open an issue before starting anything large.
Every change must pass `cargo fmt`, `clippy -D warnings` and the tests;
changes to serving behavior should also pass the comparison suites against
`next start`. [Testing](engineering/testing.md) explains how to run them.
Report security issues privately, as [SECURITY.md](SECURITY.md) describes.

## License

Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE),
at your option. Release archives and the npm package also carry the license
and notice texts of every bundled third-party component
(`THIRD-PARTY-LICENSES.txt`), including V8 and the Deno JavaScript compiled
into the binary.

NextRS is an independent project. It is not affiliated with, endorsed by or
supported by Vercel or the Next.js team. "Next.js" is a trademark of
Vercel, Inc.
