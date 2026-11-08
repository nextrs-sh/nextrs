# Configuration

The `nextrs` command line in full. `nextrs --help`, `nextrs inspect --help`
and `nextrs start --help` print the same options with their defaults.

- [Commands](#commands)
- [Building through the Deployment Adapter](#building-through-the-deployment-adapter)
- [Serving](#serving)
- [Runtime limits](#runtime-limits)
- [Outbound network](#outbound-network)
- [Cache](#cache)
- [Image optimization](#image-optimization)
- [Reverse proxies](#reverse-proxies)
- [Metrics](#metrics)

## Commands

```text
nextrs inspect <PATH> [--json]    Report what a build contains and what NextRS can serve
nextrs start <PATH> [OPTIONS]     Serve a build
```

`<PATH>` is the application directory (the one containing `.next`).
`cargo run --release --bin nextrs -- <command>` works in place of
`./target/release/nextrs`.

`nextrs inspect` on the [`next-app`](../fixtures/next-app/) fixture:

```text
NextRS Inspector

Next.js version:  16.4.3 (from .next/diagnostics/framework.json)
Metadata source:  internal .next manifests (undocumented)
App Router:       detected
Pages Router:     not detected

Routes:
  /               page           prerendered  (stored: html + rsc + 3 segments)
  /_global-error  page           prerendered  (framework; stored: html + rsc + 3 segments)
  /_not-found     page           prerendered  (framework; stored: html + rsc + 3 segments)
  /api/health     route handler  dynamic
  /books/[id]     page           dynamic  (params: nxtPid)
  /dynamic        page           dynamic
  /request-info   page           dynamic
...
```

Add `--json` to get the full inspection as JSON.

## Building through the Deployment Adapter

NextRS reads `.next` either from Next's internal manifests or from the output
of its [Deployment Adapter](https://nextjs.org/docs/app/api-reference/adapters)
([`packages/next-adapter`](../packages/next-adapter/)). The adapter is enabled
with an environment variable
([`adapterPath` / `NEXT_ADAPTER_PATH`](https://nextjs.org/docs/app/api-reference/config/next-config-js/adapterPath)),
so the application's `next.config` stays unchanged. `NEXT_ADAPTER_PATH` must
be an absolute path:

```bash
ADAPTER="$PWD/packages/next-adapter/index.mjs"
cd fixtures/next-app
NEXT_ADAPTER_PATH="$ADAPTER" npx next build
cd ../..
```

When `.next/nextrs/build-output.json` exists, NextRS reads it. Pass
`--ignore-adapter-output` to read the internal manifests instead.

## Serving

| Option | Default | Purpose |
|---|---|---|
| `--host` | `127.0.0.1` | Bind address. Loopback by default for safety; a non-loopback address logs a warning |
| `--port`, `-p` | `3000` | Port |
| `--js-runtime` | `none` | `none` answers `501` for dynamic routes; `embedded-v8` executes them |
| `--js-env NAME` | none | Expose one host environment variable to JavaScript (repeatable). Nothing else is inherited; `NODE_ENV` is always `production` |
| `--allow-unsupported` | off | Start despite unsupported features. This does **not** add those features |
| `--ignore-adapter-output` | off | Read internal manifests even if adapter output exists |

## Runtime limits

With `--js-runtime embedded-v8`. Durations are in seconds; `0` disables a
timeout.

| Option | Default | Purpose |
|---|---|---|
| `--isolates` | `1` | V8 isolates, each with its own thread, heap and event loop. Each extra isolate costs memory (on the fixture, about 10 MiB idle and 100-130 MiB under load) |
| `--max-concurrent` | `128` | Requests executing at once in one isolate |
| `--max-queued`, `--queue-timeout` | `512`, `10` | Requests waiting for an isolate, and for how long; beyond either, `503` with `Retry-After` |
| `--request-timeout` | `60` | Deadline per dynamic request, including `after()` work: `504`, or an aborted response if it already started |
| `--js-stall-timeout` | `10` | Terminate and replace an isolate whose JavaScript runs this long without yielding (affects every request in it) |
| `--js-bootstrap-timeout` | `60` | Longest an isolate may take to start (loading Next's bundles); not judged by the stall limit. On the first start, expiry fails startup; later, the isolate is restarted with backoff |
| `--js-heap-limit` | V8 default | Heap limit per isolate, in MiB; reaching it replaces the isolate. Lower values trade throughput for memory |
| `--response-buffer` | `64` | Response KiB buffered per request before the renderer waits for the client |
| `--max-request-body` | `10` | Request body limit in MiB (`413`), whether a length is declared or the body is chunked |
| `--max-connections`, `--write-timeout` | `1024`, `30` | Open connections; close clients that stop reading |
| `--shutdown-grace` | `10` | On SIGINT/SIGTERM, time for in-flight requests to finish |
| `--metrics-addr` | off | See [Metrics](#metrics) |
| `--v8-flags` | none | Experimental: V8 flags for tuning experiments (e.g. `"--max-semi-space-size=8"`). Unknown flags are refused; some flags weaken the sandbox |

## Outbound network

Denied unless `--net-allow` is given. The same policy covers server-side
`fetch`, external rewrites and remote images.

| Option | Default | Purpose |
|---|---|---|
| `--net-allow ORIGIN` | none | A destination JavaScript may fetch, and an external rewrite may forward to (repeatable): `api.example.com` (https, port 443 only), `api.example.com:8443`, `http://host:port`, or `*.example.com` (subdomains). Fetch redirects must stay within these |
| `--net-allow-addr CIDR` | none | A non-public address or range allowed hosts may resolve to (repeatable), e.g. `127.0.0.1` for a local API. Loopback, private, link-local (cloud metadata) and other special ranges are refused otherwise |
| `--net-timeout`, `--net-body-timeout` | `30`, `30` | Time to the response head (including redirects and the request body); longest wait for the next body chunk |
| `--net-max-response` | `32` | Response body limit per fetch in MiB, after decompression |
| `--net-ca-file PEM` | none | Certificate authorities trusted beside the built-in roots (repeatable), as `NODE_EXTRA_CA_CERTS` does for `next start`: a private CA for internal API or image hosts. Certificates are always verified; there is no option to turn verification off |

```bash
./target/release/nextrs start ./my-app --js-runtime embedded-v8 \
  --net-allow api.example.com --net-allow "*.cdn.example.net"
```

## Cache

The fetch cache, `unstable_cache`, ISR pages and route output, and
`"use cache"` entries, shared by all isolates.

| Option | Default | Purpose |
|---|---|---|
| `--cache-dir DIR` | none (memory only) | Persist the cache (directory created with mode 0700). Must be outside `.next` and `node_modules`. Entries are namespaced by build ID and JavaScript environment |
| `--cache-max-memory`, `--cache-max-disk` | `50`, `1024` | MiB of entries kept in memory and on disk; least recently used entries are evicted |
| `--cache-max-entry` | `2` | Larger data-cache values (`fetch`, `unstable_cache`) are not cached (MiB) |
| `--cache-max-response` | `16` | Larger ISR pages, route output and `"use cache"` values are not cached (MiB) |
| `--no-shared-cache` | off | Use Next's default handlers instead: separate per isolate, memory only. Refused with more than one isolate when the build has ISR or Server Actions |

## Image optimization

`/_next/image` needs no executor for public files and static imports.

| Option | Default | Purpose |
|---|---|---|
| `--image-concurrency N` | half the CPUs | Transformations at once, each on its own thread (never on the request threads or an isolate) |
| `--image-max-queued N` | 64 | Transformations that may wait; more answer `503` with `Retry-After` |
| `--image-max-input-pixels N` | 64,000,000 | Larger sources (width × height, read before decoding) are served unchanged, as `next start` does when sharp refuses them |
| `--image-timeout SECONDS` | the build's `imgOptTimeoutInSeconds` (7) | Then the source is served unchanged |
| `--image-memory-budget MIB` | 1024 | Memory transformations may hold at once. Each reserves what its source's header says it will allocate before it starts, and waits in the queue otherwise; work larger than the whole budget is served unchanged |
| `--image-source-budget MIB` | 512 | Source image bytes held in memory at once (downloading, waiting, being transformed); beyond it, new image work answers `503` |
| `--malloc-mmap-threshold KIB` | 1024 | glibc's mmap threshold, set at startup so that freed image buffers leave the process's memory; `0` keeps glibc's adaptive default; `MALLOC_MMAP_THRESHOLD_` or `GLIBC_TUNABLES` take precedence |
| `--image-cache-dir DIR` | none (memory only) | Persist optimized images (mode 0700, outside the project); entries are per build, checksummed, written atomically |
| `--image-cache-max-memory MIB` / `--image-cache-max-disk MIB` | 64 / 1024 | Cache sizes (the build's `images.maximumDiskCacheSize` applies when lower) |

Remote images are fetched only when `images.remotePatterns` (the
application) **and** `--net-allow` (the operator) both allow the URL, and
every redirect is checked again. A host resolving to a private or loopback
address also needs `dangerouslyAllowLocalIP` and `--net-allow-addr`; every
address the host resolves to must be allowed (`localhost` resolves to both
`127.0.0.1` and `::1`). With an absolute `assetPrefix`, static imports are
fetched from that origin, which `--net-allow` must include.

## Reverse proxies

Next.js trusts `X-Forwarded-Host` and `X-Forwarded-Proto` (for the Server
Action origin check, redirects and absolute URLs).

| Option | Default | Purpose |
|---|---|---|
| `--trusted-proxy CIDR` | none | Address or range of a reverse proxy (repeatable). Only connections from these addresses keep `X-Forwarded-*`, `X-Real-IP` and `Forwarded`; they are removed from every other request |
| `--strip-forwarded-headers` | off | Remove those headers from every request (no proxy in front) |

Without either option the headers are passed through from every client, as
with `next start`, so the proxy in front must overwrite them. With a chain of
proxies, list the one that connects to NextRS. `--trusted-proxy` never makes
an `Origin` acceptable that Next would refuse; it only decides whose
forwarded headers Next sees.

## Metrics

`--metrics-addr ADDR` serves `/metrics` (Prometheus text), `/metrics.json`
and `/healthz` on a separate address: counters, per-isolate state, heap
gauges and the first line of the last isolate failure. Keep it on loopback;
see [security.md](security.md).
