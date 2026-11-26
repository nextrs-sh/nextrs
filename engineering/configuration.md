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

Next.js version:  16.4.6 (from .next/diagnostics/framework.json)
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

For the Pages Router, NextRS reads routes and stored output from the same
manifests `next start` reads, for both kinds of build, and compares them
with the adapter's output at startup; if they disagree it refuses to start
(`pages-adapter-mismatch`). An adapter build keeps its `getStaticProps`
output under `.next/server/route-cache`; NextRS uses exactly the files the
build wrote there.

## Serving

| Option | Default | Purpose |
|---|---|---|
| `--host` | `127.0.0.1` | Bind address. Loopback by default for safety; a non-loopback address logs a warning |
| `--port`, `-p` | `3000` | Port |
| `--js-runtime` | `none` | `none` answers `501` for dynamic routes; `embedded-v8` executes them |
| `--js-env NAME` | none | Expose one host environment variable to JavaScript (repeatable). Nothing else is inherited; `NODE_ENV` is always `production`, and `NEXT_DEPLOYMENT_ID` is the build's `deploymentId` (empty without one), as `next start` sets it. Only a build with `experimental.runtimeServerDeploymentId` reads `NEXT_DEPLOYMENT_ID` from `--js-env`, and refuses to start without it |
| `--allow-unsupported` | off | Start despite unsupported features, including a build of an unsupported Next.js major (see [compatibility.md](compatibility.md#tested-versions-and-fixtures)). This does **not** add those features |
| `--log-format` | `text` | `text`, or `json` (one object per line on stderr). `NEXTRS_LOG_FORMAT=json` sets the default; levels come from `RUST_LOG` (default `nextrs=info`) |
| `--ignore-adapter-output` | off | Read internal manifests even if adapter output exists |

## Response compression

NextRS compresses responses as `next start` does, with the same switch:
`compress` in `next.config` (default `true`). Set `compress: false` and
rebuild to turn it off for both servers. That is useful for controlled
benchmarks, or behind a reverse proxy or CDN that compresses. No
command-line flag turns it off; `--compression-cache` only sizes the cache
of compressed copies described below.

- **Encoding:** gzip, at zlib's default level, when the request's
  `Accept-Encoding` makes `next start`'s middleware choose gzip. A client
  that accepts only deflate, or nothing, gets the response uncompressed.
  Brotli and zstd are not offered (nor does `next start` offer them).
- **What is compressed:** compressible types only (HTML, RSC payloads,
  JSON, JavaScript, CSS, SVG and other text), from 1 KiB up when the size
  is known; streamed responses of unknown size always. Never: images,
  fonts in WOFF/WOFF2, archives, responses that already have a
  `Content-Encoding`, `Cache-Control: no-transform`, HEAD, 204/304, and
  partial (206) responses.
- **Headers:** `Vary: Accept-Encoding` is added to every compressible
  response, compressed or not, after any `Vary` the response has (the RSC
  fields are kept). A compressed response has no `Accept-Ranges`, and its
  strong ETag becomes weak (`W/"…"`); `If-None-Match` still revalidates it
  with a `304`. A response compressed per request is streamed without a
  `Content-Length`; one served from the cache (below) has one.
- **Compression cache:** responses whose bytes cannot change are compressed
  once and the compressed copy is reused: files under `/_next/static`, and
  prerendered App Router pages (HTML, RSC payloads, segment prefetches)
  served from the build. Pages using ISR, Partial Prerendering, Draft Mode
  or revalidated since the build, all dynamic output (SSR, route handlers,
  Server Actions, streamed RSC), `public/` files and image optimizer
  results are compressed per request and never cached. A `/_next/static`
  file is keyed by its path and what the per-request `stat` reports (size,
  modification and change times, device and inode), so a file replaced on
  disk is compressed again. `--compression-cache MIB` (default `32`) bounds
  the memory; the least recently used copies are dropped beyond it, bodies
  over 4 MiB are not cached, and `0` turns the cache off. Concurrent first
  requests for one response compress it once. Hits, misses, evictions and
  the bytes held are on the metrics endpoint
  (`nextrs_compression_cache_*`).
- **Sizing the cache:** the cache helps only when it holds the gzip copies
  of the responses clients keep asking for. The gzip size of the build's
  JavaScript and CSS is an upper bound for `/_next/static`:
  `find .next/static -type f \( -name '*.js' -o -name '*.css' \) -size -4194305c -exec gzip -6 -c {} + | wc -c`
  (add the prerendered pages if they are large). The default 32 MiB holds
  that for most applications. If `nextrs_compression_cache_evictions`
  rises about as fast as `nextrs_compression_cache_misses` while
  `nextrs_compression_cache_hits` barely moves, the cache is smaller than
  the set in use. Every gzip request then compresses its response again,
  bodies over 64 KiB on the blocking thread pool. Throughput is close to
  that of `--compression-cache 0` on a mixed workload and lower on static
  files alone, and the process memory is much higher.
  The native allocator keeps the memory freed by the many pool threads:
  `nextrs_malloc_free_bytes` and the resident memory rise while
  `nextrs_malloc_in_use_bytes` stays flat. In a 90-minute test with a
  1 MiB cache against a 3.4 MiB gzip set, resident memory reached about
  870 MiB, against about 185 MiB with the cache off and about 220 MiB with
  the default size in 20-minute runs (`benchmarks/results/rc3`). It was still rising slowly
  when the test ended. Raise the size above the set in use, or set `0`.
  Lowering `--malloc-mmap-threshold` does not avoid it, and costs
  throughput.
- **Streaming:** each chunk a route writes is compressed and flushed
  before the next, so streamed HTML, RSC and Partial Prerendering responses
  arrive as progressively as without compression.
- **Scope:** only responses to clients. Next's requests to its own server
  (image sources, Server Action forwarding) are not compressed.
- **Secrets next to reflected input:** compression can leak a secret through
  response sizes when it shares a response with input an attacker controls
  (BREACH), with either server. Send such a response with
  `Cache-Control: no-transform`, or set `compress: false`.

## Byte ranges

Files under `/_next/static` and `public/` answer a single byte range with
`206 Partial Content`, as `next start` does, and advertise
`Accept-Ranges: bytes`. Prerendered pages and dynamic routes ignore
`Range` (as in `next start`). Where `next start` (the `send` package
behind Next's error handling) answers with an error page or an invalid
partial response, NextRS follows RFC 9110:

- an unsatisfiable range (`bytes=<size>-`, `bytes=-0`) is a `416` with
  `Content-Range: bytes */<size>` (`next start`: `500`);
- a malformed range (`bytes=abc`, `bytes=5-2`) is ignored: the full `200`
  (`next start`: `500`); several ranges also get the full `200`, as in
  `next start` (no `multipart/byteranges`);
- a suffix range longer than the file selects the whole file (`206`;
  `next start`: `500`);
- `HEAD` ignores `Range` and describes the full response (`next start`:
  `206` headers);
- `If-Range` uses the strong comparison: the weak ETag static files carry
  never matches, and a date matches only the exact `Last-Modified`
  (`next start` matches the weak ETag, and any later date);
- a partial response is always the uncompressed bytes, whatever
  `Accept-Encoding` says (`next start` gzips the partial body, so its
  `Content-Range` no longer describes what it sends). When the full
  response would be gzip, an `If-Range` is treated as not matching, so a
  download resumed from a compressed copy restarts in full;
- `If-None-Match` and `If-Modified-Since` are evaluated before `Range`: a
  `304` whatever the range.

Only the requested bytes are read from disk.

## Runtime limits

With `--js-runtime embedded-v8`. Durations are in seconds; `0` disables a
timeout.

| Option | Default | Purpose |
|---|---|---|
| `--isolates` | `1` | V8 isolates, each with its own thread, heap and event loop. Each extra isolate costs memory (on the fixture, about 10 MiB idle and 100-130 MiB under load) |
| `--max-concurrent` | `128` | Requests executing at once in one isolate |
| `--max-queued`, `--queue-timeout` | `512`, `10` | Requests waiting for an isolate, and for how long; beyond either, `503` with `Retry-After` |
| `--request-timeout` | `60` | Deadline per dynamic request, including `after()` work: `504`, or an aborted response if it already started |
| `--js-stall-timeout` | `10` | Terminate and replace an isolate whose JavaScript runs this long **on the CPU** without yielding (affects every request in it). A turn that is blocked rather than computing (a slow disk, page faults, CPU throttling) is logged and ended only after 6 times this in wall time. Without a per-thread CPU clock (non-Linux), wall time decides |
| `--js-bootstrap-timeout` | `60` | Longest an isolate may take to start (loading Next's bundles); not judged by the stall limit. On the first start, expiry fails startup; later, the isolate is restarted with backoff |
| `--js-heap-limit` | V8 default | Heap limit per isolate, in MiB, or `auto` (see [Memory](#memory)); reaching it replaces the isolate. Lower values trade throughput for memory |
| `--response-buffer` | `64` | Response KiB buffered per request before the renderer waits for the client |
| `--max-request-body` | `10` | Request body limit in MiB (`413`), whether a length is declared or the body is chunked |
| `--compression-cache` | `32` (`8` with `--profile low-memory`) | MiB of gzip copies of immutable responses (see [Response compression](#response-compression)); `0` compresses every response per request |
| `--max-connections`, `--write-timeout` | `1024`, `30` | Open connections; close clients that stop reading |
| `--shutdown-grace` | `10` | On SIGINT/SIGTERM, time for in-flight requests to finish |
| `--metrics-addr` | off | See [Metrics](#metrics) |
| `--ready-min-isolates` | `1` | `/readyz` reports ready only with at least this many ready isolates |
| `--v8-flags` | none | Experimental: V8 flags for tuning experiments (e.g. `"--max-semi-space-size=8"`). Unknown flags are refused; some flags weaken the sandbox |
| `--profile` | none (`performance`) | Presets for the memory settings below; see [Memory](#memory) |
| `--memory-reclaim-idle` | `0` (off) | Seconds without requests after which every isolate collects garbage and the native allocator returns its free memory, once per idle period |
| `--memory-pressure-reclaim` | `0` (off) | Percentage of the container's memory limit at which the same happens under load (at most every 10 s) |
| `--malloc-arena-max` | `0` (glibc default) | glibc malloc arenas; `MALLOC_ARENA_MAX` in the environment wins |

## Memory

Two budgets apply and they are different things:

- the **container's memory limit** (cgroup) covers the whole process: the
  V8 heaps, the native heap that Rust and V8 allocate from, thread stacks
  and the page cache of files read. The kernel enforces it by killing the
  process (OOM);
- a **V8 heap limit** covers one isolate's JavaScript heap. NextRS
  enforces it by replacing the isolate.

By default V8 sizes each heap from the machine's memory and does not see a
container limit: in a small container the heap can grow past what the
container allows, and the process is OOM-killed before V8 collects
garbage harder. `next start` does not have this problem because Node.js
sizes its heap from the container limit.

- `--js-heap-limit auto` sizes each isolate's heap as V8 would on a machine
  with the container's memory limit (or the machine's memory, whichever is
  lower) divided by `--isolates`. With one isolate this is what Node.js
  does. `nextrs_executor_heap_limit_bytes` reports the result.
- `--memory-reclaim-idle SECONDS`: after load, V8 shrinks its heaps on its
  own within a minute or two, but glibc keeps freed native memory in its
  arenas. After this many seconds without requests, each isolate runs a
  full, compacting collection and the allocator's free pages are released
  (`malloc_trim`). Once per idle period; the first request after it runs
  as usual.
- `--memory-pressure-reclaim PERCENT`: when the container's working set
  (usage minus inactive page cache) reaches this share of its limit, the
  same happens under load, with V8's critical memory-pressure response,
  at most every 10 s. Requests in an isolate wait for its collection, as
  for any full collection.
- `--malloc-arena-max N` caps glibc's arenas (default: 8 per CPU of the
  machine, not of the container). Fewer arenas retain less free memory.

None of these is a process memory limit: they reduce what the process
holds, and the kernel still enforces the container's limit.

| Profile | `--js-heap-limit` | `--memory-reclaim-idle` | `--memory-pressure-reclaim` | `--malloc-arena-max` | `--compression-cache` |
|---|---|---|---|---|---|
| `performance` (the default behaviour) | V8 default | off | off | glibc default | 32 |
| `balanced` | `auto` | 30 | 90 | glibc default | 32 |
| `low-memory` | `auto` | 10 | 85 | 2 | 8 |

Flags given explicitly win over the profile's values. Metrics:
`nextrs_memory_reclaims_total{trigger}`,
`nextrs_executor_memory_reclaims_total{trigger}`,
`nextrs_executor_memory_reclaim_heap_released_bytes_total`,
`nextrs_memory_reclaim_last_anon_{before,after}_bytes`,
`nextrs_cgroup_memory_{limit,working_set}_bytes`.

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
with `next start`, so the proxy in front must overwrite them. Startup logs a
warning when that default is used on a non-loopback `--host`. With a chain of
proxies, list the one that connects to NextRS. `--trusted-proxy` never makes
an `Origin` acceptable that Next would refuse; it only decides whose
forwarded headers Next sees.

## Metrics

`--metrics-addr ADDR` serves these on a separate address. Keep it on
loopback or a private network; see [security.md](security.md).

| Path | Content |
|---|---|
| `/metrics` | Prometheus text |
| `/metrics.json` | The same as JSON, plus native allocator detail |
| `/healthz` | Liveness: 200 while at least one isolate is ready |
| `/readyz` | Readiness: 503 once graceful shutdown begins, or with fewer than `--ready-min-isolates` ready isolates |

The metrics cover:

| Group | Metrics |
|---|---|
| HTTP | `nextrs_http_requests_total{class}`, `nextrs_http_response_head_seconds` and `nextrs_http_request_duration_seconds` histograms, `nextrs_http_requests_in_flight`, `nextrs_http_response_bodies_total{outcome=completed,aborted,failed}`, `nextrs_http_connections_open`/`_total`, `nextrs_http_draining` |
| Serving stages | `nextrs_http_served_total{via}`: what answered each client request (`static`, `public`, `image`, `prerendered` from memory, `executor` for Next's handler in an isolate, `redirect`, `not_found`, `other`); `nextrs_http_routing_seconds` and `nextrs_http_served_head_seconds{via}` histograms with buckets from 25 µs; `nextrs_http_proxy_runs_total` |
| Compression | `nextrs_compression_cache_*` (hits, misses, coalesced, bypassed, evictions, entries, held bytes, `fills_in_flight`, `fill_micros`); `nextrs_compression_stream_*` for responses compressed as they stream (responses, input and output bytes, input frames, encoder seconds) |
| Executor | Isolates, ready isolates, queue, rejections by reason, restarts and failures per slot, heap gauges and `nextrs_executor_full_gcs_total` per slot, execution and queue time |
| Shared cache | Hits, misses, entries, memory and storage bytes, evictions, errors by reason; `nextrs_cache_value_bytes_total` (entry bytes copied to JavaScript) and `nextrs_cache_reused_total` (lookups served from an isolate's decoded copy of an unchanged entry) |
| Images | `nextrs_image_*`: queue, in flight, transforms, rejections, memory reservations, cache |
| Process | `nextrs_process_resident_memory_bytes`, `_anon_bytes`, `_file_bytes`, `_peak_bytes`, `nextrs_process_swap_bytes`, `nextrs_process_cpu_seconds_total`, `_threads`, `_open_fds`, `_uptime_seconds`, and `nextrs_malloc_in_use_bytes`/`_free_bytes`/`_releasable_bytes` |

Every label comes from a fixed set (status class, outcome, reason, slot). No
path, host or header value is ever a label.

The metrics listener works with or without `--js-runtime embedded-v8`.
