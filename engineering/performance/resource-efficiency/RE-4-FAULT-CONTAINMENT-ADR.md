# ADR RE-4F: containing fatal JavaScript runtime failures

**Status:** proposed. **Decision:** keep the in-process isolate
architecture for now. Do not build a worker-process prototype in this
milestone. Revisit when the Linux fault tests have measured how often
process-fatal failures occur in practice.

## Context

Some V8 failures end the whole process, and the embedder cannot catch
them:

- one allocation beyond about three 64 MiB grants above the heap limit
  (`FatalProcessOutOfMemory`, exit 133; measured: 192 MiB at a 64 MiB
  limit, [RE-4-V8-HEAP-OVERSHOOT](RE-4-V8-HEAP-OVERSHOOT.md));
- a failed V8 page reservation or code-space allocation (documented in
  RE-3, not provoked);
- a native crash in V8 or a Rust dependency (SIGSEGV), or a Rust panic
  outside an isolate thread;
- a cgroup OOM kill: the kernel picks the largest process in the cgroup.

When they occur, every isolate and every in-flight request fails, the
native HTTP server goes down with them, and the container restarts.
Everything else (CPU stalls, heap limits reached gradually, uncaught
exceptions, executor-thread panics) already replaces one isolate while
the process keeps serving.

## Options

1. **In-process isolates (today).** One process: the Rust HTTP server
   plus N isolate threads sharing one address space, one data cache, one
   compression cache, one connection pool.
2. **Supervised JavaScript worker processes.** The native server stays
   in the parent. Each worker process hosts one or more isolates and
   speaks a request/response protocol over a Unix socket or pipe. The
   parent restarts a worker that dies.
3. **Other approaches within the current runtime.**
   - 3a. Lower the probability of fatal failures: admission (RE-4B), the
     ArrayBuffer budget (RE-4D), the native buffer budget (RE-4C), heap
     budgeting (RE-3).
   - 3b. Survive the process ending: a supervisor (container restart
     policy, systemd, Kubernetes) restarts it. That is already the
     deployment model.
   - 3c. A pre-forked standby process, ready to take the listening socket
     when the active one dies. This halves the outage, at the cost of a
     second full process.

## Evaluation

Two sources:

- **Measured on this branch** (`results/re4-local/process-memory.txt`):
  release binary, `fixtures/next-app`, idle after four requests, Fedora
  host, no container.

  | | Threads | RSS MiB | Anonymous MiB | File-backed MiB |
  |---|---:|---:|---:|---:|
  | no executor | 8 | 16.0 | 2.9 | 13.0 |
  | 1 isolate | 14 | 66.1 | 27.6 | 38.4 |
  | 2 isolates | 16 | 81.0 | 42.4 | 38.5 |

  The first isolate adds 24.7 MiB of anonymous memory, including the
  process-wide V8 platform and runtime; a second adds 14.8.
- **Series 1** (`memory-20261126`, rc.3 serving code, Docker Desktop VM,
  Velasia; [MEMORY-EFFICIENCY](../series1/MEMORY-EFFICIENCY.md)): +20 MiB
  idle and +150–175 MiB at peak per additional isolate under load.

The worker-process figures are **estimates** derived from these. No
worker process exists. File-backed pages (the binary) are shared through
the page cache and charged to the cgroup once, so a second process mostly
adds anonymous memory.

| Criterion | 1. In-process | 2. Worker processes | 3a/3b |
|---|---|---|---|
| Baseline memory | 1 isolate: 27.6 MiB anonymous idle | a parent without V8 (≈ 2.9 MiB anon) plus one worker (≈ the 1-isolate process): **≈ +3 MiB anon idle**, plus whatever the parent itself buffers | unchanged |
| Per extra isolate | +14.8 MiB anon idle (Series 1: +20 MiB idle, +150–175 MiB peak under load) | per worker, the first-isolate increment, 24.7 MiB anon: **≈ +10 MiB idle per extra worker**, before any caches are duplicated | unchanged |
| At 256 MiB | 1 isolate: highest anonymous peak 163 MiB, a 93 MiB margin (RE-2/3, Mac VM; `memory.current` also held 48–87 MiB of reclaimable page cache) | ≈ +3 MiB: small. Duplicating the data cache (up to 50 MiB) per worker would not be | — |
| IPC latency | none (channels between threads) | every request and response chunk crosses a socket: two copies and two context switches per chunk; streaming responses (RSC, SSR) are many chunks | — |
| Streaming | native (flow-controlled channel, `response.rs`) | needs a framed protocol with per-stream flow control | — |
| Async request context | `AsyncLocalStorage` inside the isolate; unchanged | unchanged inside the worker | — |
| Shared caches | data/ISR cache (`CacheStore`), its revalidation tags, internal self-fetches, the outbound connection pool: all shared by the isolates today | each must move to the parent behind IPC, or be duplicated per worker (memory × N, and revalidations must be broadcast) | — |
| Module state | per isolate either way | per isolate | — |
| Startup / replacement | isolate restart 91–558 ms (test bootstrap; Next.js longer), no process exec | process exec + V8 init + bootstrap: slower, but outside the parent | 3b: container restart, seconds |
| Independent limits | heap per isolate only | per process: `RLIMIT_AS`, a child cgroup each (needs delegated cgroups in the container), so a runaway worker can be OOM-killed alone | — |
| Native crash containment | none | yes, for crashes in the worker | 3b: restart |
| Portability | Linux, macOS (npm) | fork/exec + sockets: fine on Linux/macOS; per-worker cgroups are Linux-only and need cgroup delegation, which most container runtimes do not give | — |
| npm distribution | one binary | one binary (self-exec as a worker), workable | — |
| Complexity | — | high: a request protocol, flow control, cancellation, shutdown, health, cache IPC, two failure domains to test | 3a done here |

## What a worker split would buy

It turns "process-fatal" into "worker-fatal" for V8 aborts and native
crashes. It does **not** remove the cgroup OOM case unless each worker
has its own memory limit. In a single container that needs cgroup
delegation, which most container runtimes do not give. And with only
one worker (the 256 MiB configuration), the worker's death still fails
every in-flight JavaScript request. The parent could keep serving
static files and prerendered pages during the restart, the same as
option 1 does during an isolate restart.

## Decision

1. **Stay in-process.** The memory is not the obstacle at 256 MiB:
   about +3 MiB for a one-worker split, against a 93 MiB anonymous
   margin, as long as caches are not duplicated. The benefit is narrow
   for what it costs:
   - With one isolate, a dead worker still fails every in-flight
     JavaScript request. What survives is static and prerendered serving,
     and the health endpoints, during the restart.
   - Cgroup OOM kills are not contained without per-worker cgroups.
   - Every streamed chunk would cross IPC, and the shared data cache, its
     revalidations, self-fetches and the connection pool would need IPC
     or duplication.
   - The V8 aborts it would contain need single allocations beyond about
     three grants above the heap limit, which the RE-4 controls make less
     likely, though they cannot rule them out.
   - No native crash has been observed in the Series 1, RE-1 to RE-23B,
     or RE-4 runs.
2. **Reduce the probability instead** (3a). It is done in this milestone,
   opt-in: memory-aware admission, the ArrayBuffer budget, the native
   buffer budget.
3. **Rely on the supervisor** (3b) for what remains, and say so in the
   configuration docs: process-fatal failures exist; run NextRS under a
   restart policy.
4. **No prototype now.** The brief's condition for one was a credible
   path to better reliability without unacceptable memory overhead. The
   overhead is acceptable. The reliability gain at the targeted
   1-isolate configurations is not shown: how often process-fatal
   failures occur is exactly what the Linux fault tests have yet to
   measure.

## When to revisit

- The Linux fault tests ([RE-4-LINUX-BENCHMARK-RUNBOOK](RE-4-LINUX-BENCHMARK-RUNBOOK.md) §6)
  show process-fatal outcomes inside the declared workload envelope even
  with the RE-4 controls on.
- Deployments with ≥ 1 GiB and several isolates, where the per-worker cost is 1 %
  or less and losing all isolates at once costs more.
- A native crash is observed in production.

## Answer

Not yet. Its memory cost would be acceptable: about +3 MiB anonymous
for one worker, plus about 10 MiB per extra worker (estimates from
process-level measurements on this branch), provided caches are not
duplicated. What it would buy at one isolate
is narrow: static serving survives a V8 abort, but OOM kills and the
in-flight JavaScript requests do not. Against that come IPC on every
streamed chunk and a cache-sharing redesign. Decide after the Linux
fault tests show how often process-fatal failures occur inside the
declared envelope, and look again first for multi-isolate deployments
of 1 GiB or more.
