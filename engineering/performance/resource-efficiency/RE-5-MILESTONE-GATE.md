# RE-5: milestone gate

**CPU efficiency and allocation optimization.**

- Branch `feat/re-5-allocation-cpu-efficiency`, from main `8d83d03`
  (RE-4.1, PR #57, merged; every check of the PR and of main's
  post-merge run passed, including the pinned and 16.4.5 compatibility
  suites at 1 and 4 isolates).
- Wall-clock tracking issue opened: nextrs-sh/nextrs#58.
- **No default, profile, version, package or deployment changed.** The
  new memory control (`--prerender-cache`) is opt-in.
- **No container benchmark, container fault test or soak ran.** Every
  measurement below is process level on one Fedora host.

## Changes

| Commit | Change | Area |
|---|---|---|
| `7149b13` | Nested requests take a bounded overdraft slot instead of queueing behind their parents (prerequisite) | §3 of the brief |
| `b16dfc3` | Opt-in file-backed prerendered output with a bounded LRU (`--prerender-cache`); `generate_etag` without a temporary | RE-5B |
| `38e5659` | Multipart bodies parsed in place (one body copy less) | RE-5C |
| `1ddb050` | Build metadata freed after startup, memory returned once | RE-5B |
| `733d295` | Event-driven isolate heartbeat (park/unpark hooks, turn starts) instead of a 25 ms timer | RE-5F |
| `287bf3e` | Cache entry files built outside the store lock, without a value copy | RE-5D |
| `213c265` | Fixes from the independent review | – |
| `c721cb2` | Prerendered responses' Vary and content type made once | RE-5A |
| `8017043` | Measurement drivers | – |

## Verdicts

| # | Area | Verdict | Basis |
|---|---|---|---|
| 1 | Implementation readiness | **PASS WITH CONDITIONS** | Every change implemented with regression tests; the independent review found no critical or high issues, and its 7 fixable findings were fixed. Conditions: CI on the PR; owner review |
| 2 | Correctness and compatibility | **PASS WITH CONDITIONS** | 566 workspace tests and 37 ignored (fixture, child-process) tests pass; clippy clean on 1.98.0 and 1.100.0; 2 391 header-and-byte parity comparisons for the prerender store; the nested fix verified on real Server Actions, revalidation and Pages `res.revalidate`. Conditions: the Next.js compatibility suites run in CI only (not run locally); `resource_e2e` is local only, as in RE-4.1 |
| 3 | RAM efficiency | **PASS WITH CONDITIONS** (opt-in) | Idle anonymous memory −78 % at 1 000 routes, −92.5 % at 10 000 (865 → 65 MiB), −94 % at 50 000 (4.2 GiB → 245 MiB), with `--prerender-cache`; −4 % in the default mode from freeing build metadata. Conditions: process level; page-cache charging and eviction in a cgroup not measured; the default is unchanged pending the campaign |
| 4 | CPU efficiency | **PASS WITH CONDITIONS** (idle) / **FAIL** against the per-request target | Idle CPU per isolate −49 to −61 %; wakeups −72 %. CPU per request: no regression anywhere, JavaScript routes 1–6 % lower (3 rounds, overlapping ranges); the brief's 10–20 % per-request target is not met |
| 5 | Throughput and latency | **PASS WITH CONDITIONS** | Requests per second within −1.5 % to +6 % on every workload (JavaScript routes +2 to +6 %; the three small decreases are within run-to-run variation, one of them load-generator limited); p99 within ±8 %, and within +5 % on every server-limited workload. The file-backed mode adds 7.5–9 ms to a route's first read from a cold page cache (warm reads equal eager). Process level only |
| 6 | Native Linux benchmark validation | **BLOCKED** | The host's Docker is still Docker Desktop (a VM); the RE-4.1 harness and runbook are unchanged and ready; nothing ran in containers |
| 7 | Production release readiness | **BLOCKED** | Needs 6, the soaks, and the RE-23B conditions. No release work done |

## The twelve questions

**1. Which allocations were removed?**

- Per prerendered page request: 4 of 31 native allocation calls (−13 %);
  the Vary and content-type values are made once and the header map is
  sized once.
- Per multipart parse: one copy of the whole body.
- Per persisted cache set (with `--cache-dir`): one value-sized copy,
  no longer made under the store lock.
- At startup:
  - every prerendered body, with `--prerender-cache`;
  - the ETag's UTF-16 temporary (twice each page's size);
  - the retained `BuildInfo`.
- On JavaScript routes NextRS-attributable allocation is about 24 % of
  calls and about 10 % of bytes; the rest is V8. The 15–30 %
  native-bytes target is not reachable from NextRS code there.

**2. Which buffer copies remain necessary?**

- **JavaScript → Rust,** where the JavaScript buffer may be reused after
  the call: response chunks, fetch request bodies, upload chunks.
- **Multipart parts,** which become JavaScript-owned buffers.
- **The polyfill copies that make `readFileSync` and one-shot zlib
  budget-enforced.** Removing them would weaken enforcement.
- **String transcoding** (V8 ↔ UTF-8).
- See RE-5C §2 for the full table.

**3. Does large prerendered build size still drive idle memory growth?**

- **By default, yes, unchanged:** about 86.5 KiB per route here.
- **With `--prerender-cache`, only through the per-route index:** about
  4.5 KiB per route, 5 % of the eager slope. Payload memory becomes the
  configured cache size plus the page cache, which is reclaimable.

**4. Which optimizations measurably reduce CPU cost?**

- **The event-driven heartbeat:** idle −49 to −61 % per isolate, and the
  likely cause of the 1–6 % lower median CPU on JavaScript routes.
- **The prerendered header change:** −2 % instructions per prerendered
  request, within noise for CPU time.
- **The rest** are memory or structural changes, with no measurable CPU
  effect in the workloads.

**5. Did idle CPU consumption improve?**

- **Yes:**
  - 1 isolate: 1.42 → 0.73 millicores;
  - 2 isolates: 1.95 → 0.98;
  - 4 isolates: 3.35 → 1.35;
  - unchanged (already 0) without an executor.
- **What remains** is the supervisor's 50 ms loop, deliberately not
  changed (RE-5F §5).

**6. Did any optimization compromise resource enforcement?**

- **No.** The review found one weakening, which is fixed:
  - the prerender cache's contents were outside the native budget (F3);
    with a budget, every body the store hands out is now reserved.
- **The nested-request overdraft** is bounded per isolate and over the
  pool. It is never memory-gated, as nested requests were not before.
- **One documented exception:** a prerendered body larger than the whole
  native budget is read unreserved.

**7. Is committed accounting preferable to sampled accounting?**

- **Not decided by RE-5.** No container data exists. RE-4.1's model and
  process-level tests still favour `committed` with a 2 s TTL as the
  first campaign candidate.

**8. Is young-generation tuning justified?**

- **Not by RE-5's data, and not refuted.**
- With an explicit heap cap and no container memory to size from, V8
  already uses 16–32 MiB semi-spaces. `--js-young-semi-space` 4/8/16 was
  a no-op, by design.
- Setting the initial size too changed nothing; that change was reverted.
- The container-sized young generations (2–4 MiB) need the campaign.

**9. What resource limits remain independent?**

- **Each still applies on its own:**
  - V8 heap per isolate;
  - the ArrayBuffer budget;
  - the native buffer budget (now including the prerender cache);
  - image source and image work budgets;
  - the image, data/ISR, compression and prerender caches;
  - per-request ceilings and the queue.
- **Admission and reclaim** read the working set.
- See RE-5I §3 for the full list.

**10. Is V8 still the appropriate default engine?**

- **Yes.** On JavaScript routes the cost is in V8's own work, so another
  engine would trade compatibility for an unmeasured gain.
- The brief excludes alternatives.
- Nothing in RE-5 points elsewhere.

**11. What remains blocked by infrastructure?**

- **The whole native Linux campaign:** native Docker Engine, the owner's
  host terminal, about 45 h.
- **What it must cover:**
  - every container measurement, fault test and soak;
  - sampled against committed accounting;
  - `--prerender-cache` in a cgroup (page-cache charging, cold reads
    under pressure);
  - young-generation sizing at 512 MiB and 1 GiB;
  - the RE-23B soak and CPU conditions.

**12. Is NextRS ready to advance to final resource-efficiency validation?**

- **Yes, as an implementation.** There are no open critical or high
  findings, and the process-level gains are documented with raw data.
- The final validation is the container campaign above. It should
  include `--prerender-cache` (0, 4, 8) and the RE-5I candidate
  configurations.

## Owner decisions requested

1. Review the RE-5 PR (not to be merged without approval).
2. **The nested-request behaviour change** is not behind a flag. It
   changes scheduling for Next's in-process self-fetches only, bounded per
   isolate and over the pool. It needs an explicit acceptance, or a
   request to put it behind a flag.
3. **`--prerender-cache`:** keep it opt-in until the campaign, then
   choose profile values.
4. **The campaign:** as in RE-4.1, with RE-5's additions (question 11).
5. **Follow-ups**, not implemented: an event-driven supervisor (RE-5F
   §5); budgeted backing-store adoption (RE-5C §4); allocator-backed
   `TextEncoder`/`Buffer.from` (RE-5H, only with evidence); review F2, F4
   and F7.

RE-5 is complete to this gate. No further milestone was started.
