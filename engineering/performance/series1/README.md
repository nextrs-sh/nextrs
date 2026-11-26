# Series 1 performance investigation

Velasia Airways' first Railway series (2026-11-25) compared `next start`
and NextRS 0.1.0-rc.2 at 2–10 requests/s: NextRS was faster on static
assets and about 1 ms slower on RSC payloads. This investigation
reproduced both workloads locally, found why, and changed one thing.

**Verdict: IMPROVED — VALIDATED (locally).** Railway Series 2 must confirm
it in the cloud.

| Report | Contents |
|---|---|
| [BASELINE.md](BASELINE.md) | Series 1 reviewed; versions compared; the local environment; response parity; the baseline before any change |
| [RSC-LATENCY-INVESTIGATION.md](RSC-LATENCY-INVESTIGATION.md) | The RSC request path traced; the cause (a full cache-entry decode on every request); the change |
| [COMPRESSION-CACHE-INVESTIGATION.md](COMPRESSION-CACHE-INVESTIGATION.md) | The gzip cache and static path reviewed; fill and tail experiments; the 343.8 ms p99 |
| [MEMORY-EFFICIENCY.md](MEMORY-EFFICIENCY.md) | Idle, peak and recovered memory; NextRS without V8; cost per isolate |
| [CPU-EFFICIENCY.md](CPU-EFFICIENCY.md) | CPU per request on both workloads; where NextRS's CPU goes; cost modelling method |
| [OPTIMIZATION-RESULTS.md](OPTIMIZATION-RESULTS.md) | The change, measured before and after; instrumentation overhead; what was not changed and why |
| [REGRESSION-VALIDATION.md](REGRESSION-VALIDATION.md) | Checks and tests run, and their results |
| [FINAL-RECOMMENDATIONS.md](FINAL-RECOMMENDATIONS.md) | Before/after table, remaining issues, the plan for Railway Series 2 |

Raw data: [`benchmarks/results/series1/`](../../../benchmarks/results/series1/).
Drivers: [`benchmarks/series1/`](../../../benchmarks/series1/README.md).

Every number in these pages is a local measurement on one shared host
(Intel Core i5-8259U, Docker Desktop), unless it is quoted from the
Series 1 report and marked as such. None is a cloud result, and none is a
general claim about NextRS against Node.js.
