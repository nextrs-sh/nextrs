# Operations

How to run NextRS in a production-like deployment and what to do when
something goes wrong. NextRS is experimental software. The configurations
below are the ones Phase 3C tested. For every option, see
[configuration.md](configuration.md).

## Before deploying

1. Build the application with `next build` as usual (`output: 'standalone'`
   works, monorepos included).
2. Run `nextrs inspect <app>`.
   - The Next.js version must be 16.x; it is tested on 16.4.x. A build of
     another major is refused at startup.
   - Read every `[unsupported]` diagnostic.
3. List the environment variables the application reads (`--js-env NAME`
   for each) and the origins it fetches (`--net-allow`, plus
   `--net-allow-addr` for private addresses).
4. Choose the forwarded-header policy:

   | Deployment | Flags |
   |---|---|
   | Behind a proxy or load balancer | `--trusted-proxy <proxy CIDR>` |
   | Clients connect directly | `--strip-forwarded-headers` |

5. Size it.
   - `--isolates`: about one per CPU core you give to rendering.
   - `--js-heap-limit` so that `isolates × limit`, plus a few hundred MiB
     of native memory, fits the container.

## Example

```bash
nextrs start /srv/app --host 0.0.0.0 --port 3000 \
  --js-runtime embedded-v8 --isolates 4 --js-heap-limit 1024 \
  --js-env DATABASE_API_URL --net-allow https://api.internal --net-allow-addr 10.0.0.0/8 \
  --trusted-proxy 10.0.1.0/24 \
  --cache-dir /var/cache/nextrs/data --image-cache-dir /var/cache/nextrs/images \
  --metrics-addr 127.0.0.1:9100 --ready-min-isolates 2 --log-format json
```

## Probes

| Probe | Endpoint (metrics listener) | Meaning |
|---|---|---|
| Liveness | `GET /healthz` | At least one isolate is ready |
| Readiness | `GET /readyz` | Not shutting down, and at least `--ready-min-isolates` isolates ready |

The HTTP listener opens only after every isolate has loaded the build and
run the application's `instrumentation` `register()`. A configuration error
(a missing required variable, an instrumentation exception) fails startup.

## Shutdown

On SIGTERM:

1. `/readyz` turns 503.
2. New connections are refused.
3. In-flight requests get up to `--shutdown-grace` seconds (default 10).
4. The process exits.

Give the orchestrator a termination grace longer than `--shutdown-grace`.
In-flight work is lost if the process is killed. NextRS never replays a
request.

## What to watch

| Symptom | Metric | Typical cause and response |
|---|---|---|
| Overload | `nextrs_executor_rejected_total{reason="overloaded"|"queue_timeout"}`, `nextrs_executor_queued_requests`, `nextrs_image_overloaded` | More traffic than the isolates (or image workers) handle: add isolates or instances, or raise `--max-queued` only if latency allows |
| Isolate failures | `nextrs_executor_failures_total{reason}`, `nextrs_executor_restarts_total` | `cpu_stall`: a request ran JavaScript without yielding past `--js-stall-timeout`. `heap_limit`: an isolate reached `--js-heap-limit`. Find the route in the logs (`nextrs::executor` errors) |
| Memory pressure | `nextrs_process_resident_memory_anon_bytes`, `nextrs_executor_heap_used_after_gc_bytes`, `nextrs_malloc_free_bytes` | Heap after GC rising steadily is application state. Free malloc bytes are retained by the allocator, not leaked. See the Phase 3C soak results for the expected plateau. |
| Undersized compression cache | `nextrs_compression_cache_evictions`, `nextrs_compression_cache_misses`, `nextrs_compression_cache_hits` | Evictions rising about as fast as misses: the gzip copies in use do not fit, every request compresses again and resident memory grows (allocator retention). Raise `--compression-cache` or set `0`; see [Response compression](configuration.md#response-compression) |
| Slow responses | `nextrs_http_response_head_seconds`, `nextrs_http_request_duration_seconds` | Compare against upstream latency (`nextrs_fetch_duration_seconds`) |
| Where time goes | `nextrs_http_served_total{via}`, `nextrs_http_served_head_seconds{via}`, `nextrs_http_routing_seconds` | Which requests reach JavaScript (`via="executor"`: ISR and Partial Prerendering pages, dynamic routes, actions) and how long each source takes to its response head. Routing time includes the proxy (`nextrs_http_proxy_runs_total`) |
| Client disconnects | `nextrs_http_response_bodies_total{outcome="aborted"}` | Normal at a low rate |
| Broken streams | `nextrs_http_response_bodies_total{outcome="failed"}` | An application error after the response started; see logs |
| Cache problems | `nextrs_cache_errors_total{reason}` | Disk errors degrade the cache to memory. Check the cache volume |

## Runbook

| Situation | Steps |
|---|---|
| Startup refused, `unsupported-next-version` | Rebuild the application with Next.js 16.4.x. `--allow-unsupported` starts it anyway, with undefined behaviour |
| Startup fails with an application error | Usually a missing `--js-env` variable: startup names unset ones. Run the build under `next start` with the same environment to compare |
| Server Actions answer 500 behind a proxy | The public host is not reaching Next. Add the proxy with `--trusted-proxy` and make it set `X-Forwarded-Host` and `-Proto` |
| `fetch` fails in the application | The destination or its address is not allowed. Logs name the refused host. Add `--net-allow` (and `--net-allow-addr` for private ranges) |
| Repeated `cpu_stall` restarts | A route does synchronous work longer than `--js-stall-timeout`. Fix the route, or raise the limit knowing it affects every request on that isolate |
| Memory keeps rising | Compare heap after GC (application state) with anonymous RSS (native). Lower `--js-heap-limit`, `--cache-max-memory` or `--image-memory-budget`, or add `--v8-flags="--max-semi-space-size=8 --min-semi-space-size=8"` (measured: about a third less anonymous memory, and up to 10 % less peak throughput on render-heavy routes) |
| Disk full on the cache volume | Serving continues from memory; `nextrs_cache_errors_total` rises. Free space, or lower `--cache-max-disk` |
