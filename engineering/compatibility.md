# Compatibility

What NextRS serves today, how it was checked, and where it differs from
`next start`. NextRS treats `next start` as the reference: a response counts
as compatible only when it matches what the official server returns for the
same build.

- [Tested versions and fixtures](#tested-versions-and-fixtures)
- [Capability matrix](#capability-matrix)
- [What works today](#what-works-today)
- [Known gaps](#known-gaps)
- [Unsupported features](#unsupported-features)

## Tested versions and fixtures

- **Pinned baseline:** Next.js 16.4.3 (React 19.3.1). Only this version gates
  CI.
- **Also passing:** the same comparison suites on 16.4.4 and 16.4.5. CI runs
  them without gating. 16.4.0 is refused by Next's own security check.
- **App Router only**, Node.js runtime only.

Every check runs against small applications in [`fixtures/`](../fixtures/).
Only their routes have been verified. Other applications may use code paths
these fixtures do not reach.

| Fixture | Covers |
|---|---|
| [`next-app`](../fixtures/next-app/) | Static and prerendered pages, dynamic SSR, streaming, Route Handlers, request isolation, data fetching and the data cache, ISR, Server Actions |
| [`next-cache-components`](../fixtures/next-cache-components/) | `"use cache"`, `cacheLife`, `cacheTag`, Partial Prerendering shells |
| [`next-proxy`](../fixtures/next-proxy/) | `proxy.ts`, `next.config` headers, redirects and rewrites, `basePath`, `trailingSlash`, `assetPrefix`, case-sensitive routes |
| [`next-auth-proxy`](../fixtures/next-auth-proxy/) | A `jose` session proxy (the Next.js authentication guide's pattern) |
| [`next-jwks-proxy`](../fixtures/next-jwks-proxy/) | `jose` verifying tokens against an OpenID provider's JWKS |
| [`next-authjs`](../fixtures/next-authjs/) | Auth.js v5: credentials and OpenID Connect sign-in |
| [`next-intl-proxy`](../fixtures/next-intl-proxy/) | `next-intl` locale routing (`always`, `as-needed`, localized pathnames, domains) |
| [`next-platform-proxy`](../fixtures/next-platform-proxy/) | A proxy that fetches, external rewrites, `waitUntil`, `instrumentation.ts`, Draft Mode, `dynamicParams = false` |
| [`next-otel`](../fixtures/next-otel/) | OpenTelemetry through `@vercel/otel` |
| [`next-image`](../fixtures/next-image/) | `next/image` and `/_next/image` with local and remote sources |

The suites that compare these fixtures are described in
[testing.md](testing.md).

## Capability matrix

| Capability | Status | Notes |
|---|---|---|
| Static assets (`/_next/static`, `public/`) | ✅ Supported | Served from the build output, with ETags and immutable caching for hashed assets |
| Prerendered App Router pages | ✅ Supported | HTML stored by `next build` |
| Prerendered RSC payloads | ✅ Supported | Full payloads and segment prefetches, including the `_rsc` cache-busting check |
| Not-found handling | ✅ Supported | Serves the prerendered not-found page with a `404` |
| Build inspection and unsupported-feature detection | ✅ Supported | `nextrs inspect` and the startup check |
| Dynamic App Router pages (SSR, streaming) | 🧪 Experimental | Requires `--js-runtime embedded-v8`; otherwise `501` |
| Route Handlers (`route.ts`, Node.js runtime) | 🧪 Experimental | Requires `--js-runtime embedded-v8`; otherwise `501` |
| Streamed request bodies | 🧪 Experimental | Pulled as the handler reads them; `--max-request-body` limit, including chunked uploads |
| Multi-core isolate pool | 🧪 Experimental | `--isolates N`; bounded admission, supervision and restart per isolate |
| Outbound `fetch` from server code | 🧪 Experimental | Denied unless `--net-allow` names the destination; hosts must resolve to public addresses (or `--net-allow-addr` ranges); redirects re-checked |
| Next.js data cache (`fetch` cache, `unstable_cache`, `revalidateTag`) | 🧪 Experimental | One cache for all isolates; persisted across restarts with `--cache-dir` |
| `generateStaticParams` | 🧪 Experimental | Listed params are served prerendered; with the executor, other params are rendered on their first request and cached, or answer `404` with `dynamicParams = false` |
| Draft Mode (`draftMode()`) | 🧪 Experimental | Requires `--js-runtime embedded-v8`. Requests with the draft cookie go to Next's handler (on-demand rendering; invalid cookies cleared) instead of the prerender |
| `instrumentation.ts` (`register`, `onRequestError`) | 🧪 Experimental | Registered by Next's compiled handlers and proxy before they run: once per isolate (N registrations with `--isolates N`) |
| Server Actions (forms, client calls, bound and closure actions, uploads, redirects, cookies) | 🧪 Experimental | Requires `--js-runtime embedded-v8` (otherwise a startup refusal); Node.js runtime only; also on ISR pages; not with interception routes |
| `proxy.ts` / `middleware.ts` (Node.js runtime) | 🧪 Experimental | Requires `--js-runtime embedded-v8` (otherwise a startup refusal). Next's compiled proxy runs in the isolate pool before every matched request, prerendered and cached pages included: redirects, rewrites, request and response headers, cookies, direct responses. A proxy error, timeout or isolate failure never falls through to the route |
| ISR (`revalidate`, on-demand `revalidatePath`/`revalidateTag`/`updateTag`) | 🧪 Experimental | Requires `--js-runtime embedded-v8`. Next's handler regenerates; pages are stored in the shared cache (persisted with `--cache-dir`) and seen by every isolate |
| Cache Components (`"use cache"`, `cacheLife`, `cacheTag`) | 🧪 Partial | `"use cache"` functions share one cache across isolates; Partial Prerendering shells are completed by Next's handler. `"use cache: private"` and custom `cacheHandlers` are untested |
| `next.config` headers, redirects, rewrites (beforeFiles, afterFiles, fallback; `has`/`missing`) | 🧪 Experimental | Applied in Rust in `next start`'s order from `routes-manifest.json`; no executor needed |
| Rewrites to another origin (config or proxy) | 🧪 Experimental | Forwarded as `next start` forwards them, only to destinations `--net-allow` permits (the same policy as `fetch`, checked for every connection); otherwise `502` (`external-rewrite-denied`) and nothing is sent. WebSocket upgrades answer `501` |
| `basePath`, `trailingSlash`, `assetPrefix`, `experimental.caseSensitiveRoutes` | 🧪 Experimental | Tested in separate fixture builds, including `basePath` with `trailingSlash` |
| Image optimization (`/_next/image`, `next/image`) | 🧪 Experimental | Answered in Rust, no executor needed for public files and static imports: Next's validation, cache keys, headers and 304s; JPEG, PNG, WebP, GIF and TIFF sources; WebP, AVIF, JPEG and PNG output; EXIF orientation and colour profiles; remote sources only where both `images.remotePatterns` and `--net-allow` allow them. AVIF/HEIF and JPEG-compressed TIFF sources are served unchanged (see [Known gaps](#known-gaps)) |
| Edge runtime routes | ⛔ Unsupported | Answers `501` |
| Edge-runtime middleware | ⛔ Unsupported | Startup refusal |
| Next's `i18n` config | ⛔ Unsupported | Startup refusal. Locale routing with `next-intl`'s middleware (no `i18n` config) works |
| Pages Router | ⛔ Unsupported | Startup refusal |
| Interception routes (with the executor) | ⛔ Unsupported | Answers `501` |

Every refusal is a `501` response with an `x-nextrs-unsupported` header that
names the missing feature. NextRS returns this rather than an answer that
might be wrong. Without `--js-runtime embedded-v8`, every dynamic route
answers `501` with `x-nextrs-unsupported: javascript-required`, and a build
with Server Actions, ISR or a proxy is refused at startup (unless
`--allow-unsupported` is given).

## What works today

- **Build output inspection:** `nextrs inspect` reports the Next.js version,
  routes, how each one is rendered, what is stored for it, and diagnostics.
- **Static and prerendered serving:** hashed assets, `public/` files,
  prerendered HTML, RSC payloads and segment prefetches, and the not-found
  page, served from Rust.
- **Experimental dynamic rendering:** with `--js-runtime embedded-v8`, the
  fixtures' dynamic pages and Route Handlers render through Next's
  unmodified compiled handlers. Responses stream, and request-scoped state
  (`headers()`, `cookies()`, params, `React.cache`, `after()`) stays
  isolated between concurrent requests.
- **A bounded, supervised executor:** a configurable pool of V8 isolates
  (`--isolates`), each on its own thread. Admission is bounded (busy pools
  answer `503` with `Retry-After`). Responses apply backpressure to the
  renderer, so a slow client cannot grow server memory. Request bodies
  stream into the handler, with a size limit for any framing. A failed or
  stuck isolate is terminated and replaced while the others keep serving.
- **Outbound fetch, as a capability:** with `--net-allow`, server code can
  call allowed APIs. Addresses are checked after DNS resolution by the same
  resolver the client connects with, so DNS tricks, redirects and encoded
  IP literals cannot reach loopback, private networks or cloud metadata
  services. Responses stream with backpressure, and every fetch is bounded
  by timeouts and a size limit and aborted when its request ends.
- **A shared data cache:** cached `fetch` results and `unstable_cache`
  entries live in one Rust store for every isolate, follow Next's
  revalidation and tag rules, and persist across restarts with
  `--cache-dir`. JavaScript never gets file access to it.
- **ISR and Cache Components:** time-based and on-demand ISR, unlisted
  `generateStaticParams` params and ISR route handlers are regenerated by
  Next's own handler and stored in the same shared cache, so every isolate
  serves the same generation and the cache survives restarts with
  `--cache-dir`. A page missing in one isolate while another regenerates it
  waits for that result. `"use cache"` functions share one cache too.
- **Server Actions:** form actions (with and without JavaScript), Server
  Functions called from client code, bound arguments and encrypted
  closures, file uploads, redirects, cookies and errors behave as they do
  under `next start`. Next's compiled handler does the work: NextRS routes
  the requests and provides the Web APIs it needs (`FormData`, multipart
  parsing, `crypto.subtle`). Mutations are never retried automatically.
- **Web Crypto for authentication libraries:** ECDSA (P-256, P-384),
  RSASSA-PKCS1-v1_5, RSA-PSS and Ed25519 signatures; `raw`, `spki`, `pkcs8`
  and `jwk` keys; HMAC, HKDF, AES-GCM and AES-CBC. Results match Node's on a
  shared test file. Signatures use `ring`, key formats the RustCrypto
  format crates, and key material stays in Rust.
- **Proxy and routing rules:** an unchanged `proxy.ts` runs as Next compiled
  it, at the point in the pipeline where `next start` runs it (after
  `next.config` headers and redirects, before rewrites, files, prerendered
  pages, ISR, routes and Server Actions). `next.config` headers, redirects
  and rewrites, `basePath` and `trailingSlash` are applied from Next's own
  compiled rules; the routing functions are checked against Next's own on
  thousands of generated cases. Tested with real proxies: `jose` session
  verification, `jose` verifying RS256, PS256, ES256, ES384 and EdDSA tokens
  against an OpenID provider's JWKS (key rotation, provider failures),
  Auth.js v5 (credentials and OpenID Connect sign-in, encrypted JWT
  sessions, its `authorized` callback as the proxy), `next-intl`'s locale
  middleware in all its routing modes, a proxy that fetches, rewrites to
  other origins and schedules `waitUntil` work, and OpenTelemetry through
  `@vercel/otel`.
- **Image optimization:** `/_next/image` is answered in Rust with Next's
  request semantics, memory-safe Rust decoders with resource limits, local
  sources resolved through routing, remote sources under the network
  policy, and an image cache.
- **Unsupported-feature detection:** `nextrs start` refuses builds it would
  serve incorrectly, instead of silently skipping middleware, redirects or
  security headers.

## Known gaps

- **Dynamic SSR beyond the fixtures:** only the fixtures' routes have been
  verified.
- **Execution limits are per isolate, not per request:** many requests share
  an isolate. A request that keeps the CPU busy past `--js-stall-timeout`
  is stopped by terminating its **whole isolate**, which fails the other
  requests in it. Per-request deadlines (`--request-timeout`) cancel the
  request but do not interrupt running JavaScript.
- **Caching limits:** one cache directory serves one process (no sharing
  between processes or machines). Concurrent regenerations of one page are
  coalesced across the isolates of a process, not across processes. Each
  ISR request runs Next's handler in JavaScript, even for a cache hit. The
  build-time fetch cache (`.next/cache/fetch-cache`) is not reused. Cache
  entries and revalidated tags survive a restart with `--cache-dir`, where
  `next start` forgets its tags and `"use cache"` entries. Module state is
  per isolate.
- **Outbound fetch gaps:** no `data:` URLs, custom CAs, client certificates,
  proxies or HTTP/2. A fetch still open when its request ends is detached
  (a shared or background fetch completes) and aborted after a grace as
  long as the request's deadline; a response body nobody started to read
  is released when its request's handler finishes.
- **Node.js API coverage:** NextRS provides a narrow Node.js layer. Node's
  network modules (`http`, `https`, `net`, `dns`; use `fetch`), `zlib`,
  `child_process`, `worker_threads`, `vm` and native addons are
  unavailable, and so are database drivers that need TCP sockets (Auth.js
  database adapters, email providers). Web Crypto has no ECDH, RSA-OAEP,
  AES-KW, PBKDF2, `deriveKey` or key wrapping, no P-521 (ES512), no RSA key
  generation, and refuses RSA keys under 2048 bits.
- **Proxy limits:** edge-runtime middleware, `skipProxyUrlNormalize` and
  Next's `i18n` config are refused. A request body that reaches the proxy
  is buffered for the proxy and the route; above `proxyClientMaxBodySize`
  (10 MB by default) NextRS answers `413`, where `next start` truncates it.
  Each proxied request takes one more trip through an isolate. External
  rewrites do not forward hop-by-hop headers or Next's internal headers,
  and do not proxy WebSockets. `waitUntil` work ends with its request's
  deadline. Only the fixtures' proxies and rules have been verified.
- **Image optimization gaps:** AVIF/HEIF and JPEG-compressed TIFF sources
  are served unchanged (no decoder), where `next start` converts them.
  JPEG output is about a third larger than sharp's (mozjpeg) and PNG output
  is full colour (sharp quantizes); WebP, the default output, is equivalent.
  AVIF encoding is about 2.5× slower than sharp's. JPEGs are decoded at full
  size, so large sources need more memory than in `next start`. The image
  cache is in memory unless `--image-cache-dir` is given;
  `images.customCacheHandler` is ignored. Remote images need `--net-allow`
  (and, for loopback or private addresses, both `dangerouslyAllowLocalIP`
  and `--net-allow-addr`).
- **Other framework features:** response compression is not implemented.
- **Browser verification:** Server Actions, proxies, real-world proxy
  applications and `next/image` are tested in Chromium against
  `next start`. The other fixture pages are compared by their HTML and RSC
  bytes only.

## Unsupported features

| Feature | Behavior |
|---|---|
| Pages Router | Startup refusal |
| Next's `i18n` config | Startup refusal (use `next-intl`'s middleware instead) |
| Edge-runtime middleware | Startup refusal |
| Edge runtime routes | `501` |
| Server Actions on the edge runtime | Startup refusal |
| Interception routes | `501` |
| `skipProxyUrlNormalize` with a proxy | Startup refusal |
| WebSocket upgrades on external rewrites | `501` |

`--allow-unsupported` starts a build despite a refusal. It does **not**
implement the missing feature; see [security.md](security.md#unsupported-feature-bypass).
