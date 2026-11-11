# Roadmap

NextRS is developed in phases. Each phase ends with a verdict based on the
comparison suites, soaks and CI, not on a date. No release dates are planned,
and scope changes as the experiments produce evidence.

## Completed and experimental phases

| Phase | Scope | Status |
|---|---|---|
| **0: build output feasibility** | Read `.next` through the adapter API and internal manifests; serve static and prerendered output from Rust; unsupported-feature detection; comparison harness against `next start` | ✅ Completed |
| **1A: embedded JavaScript execution** | Run Next's compiled App Router handlers in embedded V8 with no Node.js process; streaming; request isolation; engine evaluation (V8, QuickJS, JavaScriptCore, Deno, Bun) | ✅ Completed |
| **1B: executor hardening** | Response backpressure; executor supervision and restart; request deadlines and a CPU stall watchdog (per isolate); streamed request bodies; a multi-core isolate pool with bounded admission; metrics; CI against several Next.js 16.4.x versions | ✅ Completed |
| **1C: capabilities and validation** | Outbound `fetch` as an explicit, SSRF-guarded capability; Next's data cache, shared by every isolate and optionally persisted; long soaks | 🧪 Experimental (6-hour soak at 4 isolates passed; 1 and 2 isolates not re-soaked) |
| **1D: Server Actions** | Server Actions and Server Functions through Next's compiled handlers: routing, `FormData` and multipart, AES-GCM for encrypted closures, Next's requests to its own server, mutation failure semantics, browser tests | 🧪 Experimental |
| **1E: regeneration** | ISR through Next's handler with regenerated pages in the shared cache, single-flight regeneration across isolates, tag revalidation, Server Actions on ISR pages, Cache Components (`"use cache"`), a trusted reverse proxy mode | 🧪 Experimental |
| **2A: proxy and routing** | Next's compiled `proxy.ts` in the isolate pool, `next start`'s routing order, `next.config` headers, redirects and rewrites, `basePath`, `trailingSlash`, proxy security regression tests | 🧪 Experimental |
| **2B: real-world proxies** | `jose` and `next-intl` proxies unchanged, a proxy that fetches, external rewrites under the network policy, `waitUntil`, `instrumentation.ts`, Draft Mode, `dynamicParams = false`, configuration combinations, graceful shutdown, soaks with proxy traffic | 🧪 Experimental |
| **2C: authentication libraries** | Asymmetric Web Crypto, `jose` with a remote JWKS, Auth.js v5, `next-intl` routing modes, OpenTelemetry, `experimental.proxyTimeout`, a fetch outliving its request, independent outcome checks and negative controls, a six-hour proxy soak | 🧪 Experimental |
| **2D: images** | `/_next/image` in Rust for unchanged `next/image` applications: Next's request semantics, memory-safe decoding with resource limits, local sources through routing, remote sources under the network policy, an image cache, browser tests | 🧪 Experimental |
| **2E: image hardening** | Shrink-on-load JPEG decoding, memory-weighted admission and a source budget, allocator settings, mozjpeg JPEG and lossless palette PNG output, HTTPS sources with private CAs, `assetPrefix` builds, image soaks beside SSR | 🧪 Experimental |
| **2F: Pages Router** | `pages/` applications through Next's compiled handlers: every data function, ISR and on-demand revalidation, Preview Mode, API routes, `/_next/data`, hybrid `app/` + `pages/` builds, negative controls | 🧪 Experimental |
| **2G: framework compatibility** | Next's native `i18n` routing (sub-path and domain locales, detection, data routes, localized 404s and ISR), Pages Router builds from the Deployment Adapter (one build model, cross-checked at startup), a custom App's `getInitialProps`, Pages Router cache review, middleware prefetches, a mixed App/Pages/image/i18n soak driver, new negative controls | 🧪 Experimental (six-hour mixed soak not yet run) |

"Experimental" means the feature passes the comparison suites on the
fixtures, with the gaps listed in
[compatibility.md](compatibility.md#known-gaps).

## Candidate milestones

Drawn from the known gaps. These are directions, not commitments.

- **Per-request execution limits:** today a runaway request terminates its
  whole isolate.
- **Multi-process caching:** a cache and regeneration coordination shared
  between processes or machines.
- **Compatibility breadth:** more real-world applications and libraries,
  the build-time fetch cache, an ISR fast path that avoids JavaScript on
  cache hits.
- **Image engine:** AVIF/HEIF sources, faster AVIF output (rav1e with
  assembly), PNG output size closer to sharp's without visible loss.
- **Advanced Next.js features:** Next's `i18n` config with the App
  Router, a Pages Router `getStaticProps` cache-hit path without
  JavaScript (feasibility studied in Phase 2G, deferred).
- **Performance and production readiness:** performance work, measured only
  once correctness is established; operational hardening.
