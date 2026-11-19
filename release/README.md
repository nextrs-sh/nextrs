# NextRS v0.1.0 — installation and operation

NextRS serves the output of an official Next.js `next build` from a single
Rust binary with embedded V8. This guide covers the v0.1.0 release
candidates: installing the binary, running an application, and operating it.

> [!WARNING]
> **Experimental release.** v0.1.0 is the first release of NextRS, marked
> as a **prerelease**. It is tested on a defined scope (below) and on one
> real application. It is not a drop-in replacement for every Next.js
> application, and it has not run in production anywhere. Read
> [Known limitations](#18-known-limitations) before you deploy it.

Contents:

1. [Requirements](#1-requirements)
2. [Installation](#2-installation)
3. [Quick start](#3-quick-start)
4. [Building the application](#4-building-the-application)
5. [Running NextRS](#5-running-nextrs)
6. [Environment variables](#6-environment-variables)
7. [Outbound network allowlist](#7-outbound-network-allowlist)
8. [Reverse proxies and forwarded headers](#8-reverse-proxies-and-forwarded-headers)
9. [Caches](#9-caches)
10. [Image optimization limits](#10-image-optimization-limits)
11. [Isolates and memory](#11-isolates-and-memory)
12. [Health and readiness](#12-health-and-readiness)
13. [Metrics and logging](#13-metrics-and-logging)
14. [Graceful shutdown](#14-graceful-shutdown)
15. [Production example](#15-production-example)
16. [Troubleshooting](#16-troubleshooting)
17. [Compatibility](#17-compatibility)
18. [Known limitations](#18-known-limitations)
19. [Reporting security issues](#19-reporting-security-issues)
20. [Verifying a release](#20-verifying-a-release)

## 1. Requirements

**To run NextRS:**

- **Platform:** Linux on x86_64, with glibc 2.35 or later. This includes
  Ubuntu 22.04 and later, and Debian 12 and later. Other platforms and
  architectures have no release binary and are untested.
- **Runtime:** none. The binary needs no Rust toolchain, no Node.js and no
  Deno.
  - V8 is linked into the binary.
  - TLS uses a bundled Mozilla root store, so no system CA package is
    needed.

**To build your application:** Node.js and the official Next.js toolchain,
as usual. `next build` is not replaced.

**Supported applications:** Next.js 16.4.x, built with `next build`.

| Version | Status |
|---|---|
| 16.4.6 (React 19.3.1) | Primary tested version |
| 16.4.5 | Also tested |
| Other 16.x | Start with an `untested-next-version` warning |
| Other majors | Refused at startup |

Your application's backend services are separate. A database, an API or
an authentication server written for Node.js still runs on Node.js. NextRS
replaces only the Next.js web server process.

## 2. Installation

Download the archive and its checksum from the GitHub release, verify the
checksum, and put the binary on your `PATH`:

```bash
V=0.1.0-rc.1
curl -LO https://github.com/nextrs-sh/nextrs/releases/download/v$V/nextrs-v$V-linux-x86_64.tar.gz
curl -LO https://github.com/nextrs-sh/nextrs/releases/download/v$V/nextrs-v$V-linux-x86_64.tar.gz.sha256
sha256sum -c nextrs-v$V-linux-x86_64.tar.gz.sha256
tar -xzf nextrs-v$V-linux-x86_64.tar.gz
sudo install -m 0755 nextrs-v$V-linux-x86_64/nextrs /usr/local/bin/nextrs
nextrs --version        # nextrs 0.1.0-rc.1
```

The archive contains:

| File | Contents |
|---|---|
| `nextrs` | The binary |
| `README.md` | This guide |
| `RELEASE-NOTES.md` | The release notes |
| `BUILD-INFO.json` | Version, source commit, compiler, Cargo profile, minimum glibc, binary checksum |
| `THIRD-PARTY.md` | The statically linked crates and their licenses |

To build from source instead, see the repository README. NextRS is not
published on crates.io; the crate named `nextrs` there is an unrelated
project.

## 3. Quick start

```bash
cd my-app
npx next build
nextrs start . --js-runtime embedded-v8
```

The application is served at <http://127.0.0.1:3000>. Then run:

```bash
nextrs inspect .   # how each route will be served, and anything unsupported
```

**`--js-runtime embedded-v8` is required** for anything dynamic: server
rendering, route handlers, Server Actions, ISR, `proxy.ts` and middleware.

- **Without it, NextRS runs no JavaScript.** It serves prerendered pages
  and static files from Rust, and answers dynamic routes with `501`.
- **A build that needs JavaScript is refused at startup** without it, for
  example one with Server Actions or ISR.

Most applications also need some configuration:

- environment variables (§6);
- outbound fetch destinations (§7);
- a forwarded-header policy (§8).

Not every application runs without it.

## 4. Building the application

Use the official toolchain, unchanged.

- **Standalone output.** Builds with `output: 'standalone'` are supported.
  Point NextRS at the standalone directory, or at the project; both are
  read.
- **Monorepos.** pnpm and npm workspaces are supported. NextRS finds the
  workspace root and lets the application read its `node_modules` (read
  only).
- **Deployment Adapter (optional).** `@nextrs/next-adapter`, in
  `packages/next-adapter` of the repository, writes build metadata through
  the Next.js Deployment Adapter API. Without it, NextRS reads Next's
  internal manifests, and `nextrs inspect` says which source it used.
- **Node APIs.** Code that needs Node's network modules (`net`, `tls`,
  `http` servers), `child_process`, native addons or `zlib` streams cannot
  run inside NextRS (§18).

## 5. Running NextRS

```text
nextrs start <app> [--host 127.0.0.1] [--port 3000] --js-runtime embedded-v8 [options]
```

- **Bind address.** NextRS binds to `127.0.0.1` by default. Use `--host
  0.0.0.0` to listen on all interfaces; it then logs a warning.
- **All options.** `nextrs start --help` lists every option. The
  [configuration reference](../engineering/configuration.md) explains each
  one.
- **Read-only.** NextRS does not write to the application directory, which
  can be read-only.

## 6. Environment variables

**JavaScript sees no host environment variables by default.** Expose each
one by name:

```bash
nextrs start . --js-runtime embedded-v8 --js-env DATABASE_API_URL --js-env AUTH_SECRET
```

- `NODE_ENV` is always `production`.
- Variables inlined at build time (`NEXT_PUBLIC_*`) are already in the
  build.
- If the application validates its environment at startup, as in
  `instrumentation.ts`, a missing variable fails startup, and the error
  names it.

## 7. Outbound network allowlist

**`fetch` from application code may reach only the destinations you
allow.**

```bash
--net-allow https://api.example.com      # one origin
--net-allow '*.example.com'              # a domain and its subdomains (https)
--net-allow http://10.0.0.5:8080         # plain HTTP must be explicit
--net-allow-addr 10.0.0.0/8              # private addresses an allowed host may resolve to
```

| Rule | Behaviour |
|---|---|
| A host with no scheme | Means https |
| Loopback, private and link-local addresses | Refused unless listed with `--net-allow-addr`, even for an allowed host name (SSRF protection) |
| A refused fetch | Rejects in JavaScript, and the log names the host |
| Private certificate authorities | Add with `--net-ca-file ca.pem` (as `NODE_EXTRA_CA_CERTS` does for `next start`) |
| Limits | `--net-timeout`, `--net-body-timeout`, `--net-max-response` |

## 8. Reverse proxies and forwarded headers

**Choose a policy for every deployment.** Next.js trusts `X-Forwarded-Host`
and `X-Forwarded-Proto` for the Server Action origin check, for redirects
and for absolute URLs.

| Deployment | Use |
|---|---|
| Clients connect to NextRS directly | `--strip-forwarded-headers` |
| Behind a reverse proxy or load balancer | `--trusted-proxy <CIDR of the proxy that connects to NextRS>` |
| Behind a chain of proxies | `--trusted-proxy` with the last hop, the one that connects to NextRS |

- **With `--trusted-proxy`,** forwarded headers are kept only on
  connections from the listed addresses. They are removed from every other
  connection.
- **With `--strip-forwarded-headers`,** they are removed from every request.
- **Configure the proxy as well.** It should overwrite these headers, not
  append to them.

**Do not rely on the default.** Without either flag, forwarded headers
pass through from any client, as on `next start`. NextRS logs a warning
when you combine that with a non-loopback `--host`.

What the default allows: a non-browser client can send `Origin:
https://evil.example` together with `X-Forwarded-Host: evil.example`. That
request then passes Next's Server Action origin check, on `next start` and
on NextRS alike.

- **Browsers are not affected.** A browser cannot add that header to a
  cross-site request without a CORS preflight.
- **No victim cookies are exposed.** Clients that can add the header do
  not carry a victim's cookies.
- **Either flag closes the case.**

What the flags never do:

- Neither flag makes an `Origin` acceptable that Next.js would refuse.
- Next's internal headers are removed from every client request regardless.

**Load balancer idle timeout.** NextRS closes an idle HTTP/1.1 keep-alive
connection after 30 seconds. Give the load balancer or proxy an idle
timeout below 30 s towards NextRS. Otherwise a request it sends at that
instant can be reset.

- Browsers retry such a request.
- Proxies retry it only if they are configured to retry idempotent
  requests.

## 9. Caches

| Cache | Default | Persistent option |
|---|---|---|
| Data and response cache (fetch cache, `unstable_cache`, `'use cache'`, ISR pages) | In memory, shared by all isolates | `--cache-dir DIR` (created with mode 0700, outside the build) |
| Optimized images | In memory | `--image-cache-dir DIR` |

- **Size limits:** `--cache-max-memory`, `--cache-max-disk`,
  `--cache-max-entry`, `--cache-max-response`, `--image-cache-max-memory`,
  `--image-cache-max-disk`.
- **Caches are per instance.** Two NextRS processes do not share entries,
  and `revalidateTag` or `revalidatePath` on one does not reach the other.
  Run one instance, or accept per-instance staleness.
- **Disk errors.** A disk error, such as a full or read-only volume,
  degrades the cache to memory. The error is counted, and serving
  continues.

## 10. Image optimization limits

`/_next/image` is served natively, with no `sharp`. The defaults are safe:

| Option | Default | Effect |
|---|---|---|
| `--image-concurrency` | half the CPUs | Transformations at once |
| `--image-max-queued` | 64 | Beyond this, `503` |
| `--image-memory-budget` | 1024 MiB | Memory transformations may reserve at once |
| `--image-source-budget` | 512 MiB | Source bytes held at once |
| `--image-max-input-pixels` | 64,000,000 | Larger sources are served unchanged |
| `--image-timeout` | the build's `imgOptTimeoutInSeconds` (7 s) | Then the source is served unchanged |

The build's `images` configuration applies unchanged: `remotePatterns`,
`localPatterns`, sizes, qualities, formats and `dangerouslyAllowSVG`.

Remote image sources also need `--net-allow`.

## 11. Isolates and memory

| Option | Effect |
|---|---|
| `--isolates N` | V8 isolates for dynamic routes. Each has its own thread, heap and event loop. Use about one per core you give to rendering. |
| `--js-heap-limit MIB` | V8 heap limit per isolate. An isolate that reaches it is replaced, and its in-flight requests fail. |
| `--max-concurrent`, `--max-queued`, `--queue-timeout` | Admission: excess requests get `503` instead of unbounded queueing. |
| `--request-timeout` | Deadline per dynamic request (default 60 s). Expiry gives a `504`. |
| `--js-stall-timeout` | An isolate whose JavaScript runs this long on the CPU without yielding is replaced (default 10 s). |

**Size the container.** Allow `isolates × js-heap-limit`, plus a few
hundred MiB of native memory for caches, images and buffers.

Phase 3C reference on one test machine:

| Isolates | Steady RSS under mixed load |
|---|---|
| 4 | about 0.8–1.2 GiB |
| 1 | about 0.47 GiB |

**State is per isolate.** Module-level JavaScript state, such as an
in-memory counter or a cache in a module variable, is separate in each
isolate, as it would be in separate Node.js processes.

## 12. Health and readiness

Enable the operations listener with `--metrics-addr 127.0.0.1:9100`. Keep
it on loopback or a private network.

| Endpoint | Meaning |
|---|---|
| `GET /healthz` | Liveness: at least one isolate is ready |
| `GET /readyz` | Readiness: not shutting down, and at least `--ready-min-isolates` isolates ready |

**Startup.** The HTTP listener opens only after every isolate has loaded
the build and run the application's `instrumentation` `register()`.

## 13. Metrics and logging

**Metrics** are on the operations listener:

| Endpoint | Format |
|---|---|
| `GET /metrics` | Prometheus text |
| `GET /metrics.json` | JSON |

They cover the executor, HTTP, fetch, caches, images and process memory.
The [operations guide](../engineering/operations.md) lists the metrics
worth alerting on.

**Logs** go to stderr:

- `--log-format json` (or `NEXTRS_LOG_FORMAT=json`) gives one JSON object
  per line.
- `RUST_LOG` sets levels; the default is `nextrs=info`.
- JavaScript error messages are logged, never sent to clients.
- NextRS's own error responses are fixed texts with an `x-nextrs-error`
  code.

NextRS writes no per-request access log in v0.1.0. Use your proxy's
access log.

## 14. Graceful shutdown

On SIGTERM or SIGINT:

1. `/readyz` turns `503`.
2. NextRS stops accepting connections.
3. In-flight requests get up to `--shutdown-grace` seconds (default 10).
4. Isolates stop, and the process exits 0.

Give your orchestrator a termination grace period longer than
`--shutdown-grace`.

**Requests are never replayed.** Requests still running when the process is
killed are lost.

## 15. Production example

```bash
nextrs start /srv/app --host 0.0.0.0 --port 3000 \
  --js-runtime embedded-v8 --isolates 4 --js-heap-limit 1024 \
  --js-env DATABASE_API_URL --net-allow https://api.internal --net-allow-addr 10.0.0.0/8 \
  --trusted-proxy 10.0.1.0/24 \
  --cache-dir /var/cache/nextrs/data --image-cache-dir /var/cache/nextrs/images \
  --metrics-addr 127.0.0.1:9100 --ready-min-isolates 2 --log-format json
```

Run it as an unprivileged user that can write only the two cache
directories.

## 16. Troubleshooting

| Symptom | Cause and fix |
|---|---|
| Startup refused: `unsupported-next-version` | The build is not Next.js 16. Rebuild with 16.4.x. |
| Startup refused: features need JavaScript | Add `--js-runtime embedded-v8`. |
| Startup fails with an application error | Usually a missing `--js-env`; the error names it. Compare with `next start` under the same environment. |
| Dynamic routes answer `501` | `--js-runtime embedded-v8` is missing. |
| `fetch` fails inside the application | The destination is not allowed. The log names it. Add `--net-allow`, and `--net-allow-addr` for private addresses. |
| Server Actions answer `500` behind a proxy | The public host does not reach Next. List the proxy with `--trusted-proxy`, and make it set `X-Forwarded-Host` and `X-Forwarded-Proto`. |
| Occasional `503` under load | Admission control: isolates or the image queue are full. Add isolates or instances. Raise `--max-queued` only if latency allows. |
| `502` after an isolate restart | The request's isolate was replaced: a CPU stall, the heap limit, or a crash. The log names the reason. NextRS does not retry, so do not auto-retry mutations. |
| Connection resets through a load balancer | Its idle timeout towards NextRS is 30 s or more. Set it below 30 s. |
| Memory keeps rising | Compare heap after GC (application state) with anonymous RSS (native). See the [operations runbook](../engineering/operations.md). |

## 17. Compatibility

Tested in v0.1.0 against `next start`, on Next.js 16.4.6 and 16.4.5 on
Linux (some of these are classed *experimental* or *partial* in the
contract below):

- **App Router:** server rendering, streaming and RSC, Server Actions, ISR
  and on-demand revalidation, Cache Components and partial prerendering.
- **Pages Router:** `getServerSideProps`, `getStaticProps` with ISR, API
  routes, i18n, Preview and Draft Mode.
- **Routing and requests:** `proxy.ts` and middleware, rewrites, redirects
  and headers.
- **Images:** `next/image` optimization.
- **Integrations:** Auth.js, and `jose`-based sessions.
- **Instrumentation:** `instrumentation.ts`.

The full matrix, including partially supported, experimental and
unsupported features, is in the repository:

- `engineering/compatibility.md`;
- the release compatibility contract published with this release.

NextRS does not claim compatibility with every Next.js application.

## 18. Known limitations

1. **Node.js APIs are a subset.** Node's network modules, `child_process`,
   worker threads, native addons and `zlib` streams are unavailable.
   - Many database drivers do not work.
   - Call such services over HTTP.
2. **Web Crypto is partial.** `deriveKey`, `wrapKey`, `unwrapKey` and some
   algorithms throw an explicit error.
3. **Timers are about 1 ms later than Node's.** This comes from Tokio's
   timer granularity.
4. **Caches and JavaScript state are per instance and per isolate**
   (§9, §11).
5. **Mutations are dispatched at most once.**
   - If an isolate is replaced while a Server Action runs, the client gets
     a `502`. The mutation may or may not have been applied.
   - Use idempotency keys for effects that must happen exactly once.
6. **Forwarded headers are permissive by default** (§8).
7. **Pages Router tail latency.** Proxied Pages Router p99 is higher than
   Phase 2G's under open-loop load, a measured GC trade-off.
8. **Image transformation throughput** is about 0.67× of `sharp` on the
   measured workload. The defaults favour bounded memory.
9. **No access log or request ID** in v0.1.0.
10. **Single-machine evidence.** All performance and soak evidence comes
    from one x86_64 machine.

## 19. Reporting security issues

**Do not open a public issue for a vulnerability.** Report it privately
through GitHub's private vulnerability reporting on the repository
(Security → Report a vulnerability). See `SECURITY.md` in the repository.

## 20. Verifying a release

Verify the download against the published checksum:

```bash
sha256sum -c nextrs-v0.1.0-rc.1-linux-x86_64.tar.gz.sha256
```

`BUILD-INFO.json` records the commit, compiler and profile the binary was
built from, and the binary's own SHA-256. To build the same source
yourself:

```bash
git checkout v0.1.0-rc.1
cargo +1.98.0 build --locked --profile dist -p nextrs-cli
```

**A rebuild is not bit-identical to the published binary.** Builds are
not yet reproducible bit for bit. Two builds of the same commit with the
same toolchain differ in a few kilobytes of code. Compare the version and
commit, not the checksum.

The `dist` profile is `release` with fat LTO and one codegen unit. It sets
no CPU-specific flags, so the binary runs on any x86_64 CPU.
