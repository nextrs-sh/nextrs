# /home/dennispaler/Documents/OpenSource/nextrs/benchmarks/results/phase3c/final-bench-f5c480d/readme

Medians over valid rounds. Ratios NextRS / next-start with ✔ better, ✘ worse, · within noise (95 % bootstrap interval). Throughput, CPU and memory: closed loop; latency: open loop at an equal offered rate. ⚑ load generator limited (req/s is a lower bound).

| Workload | next-start req/s | nextrs-i1 req/s | nextrs-i4 req/s | CPU ms/req next-start → nextrs-i1 | open-loop p95 ms next-start → nextrs-i1 | anon RSS MiB next-start → nextrs-i1 |
|---|---:|---:|---:|---|---|---|
| app/ssr | 611 | 736 (×1.20 ✔) | 1,898 (×3.11 ✔) | 1.922 → 1.746 (×0.91 ✔) | 6.26 → 3.30 (×0.53 ✔) | 376 → 141 (×0.37 ✔) |

## Isolate scaling (closed-loop throughput relative to 1 isolate; efficiency = speed-up / isolates)

| Workload | nextrs-i1 | nextrs-i4 |
|---|---:|---:|
| app/ssr | 1.00× (100 %) | 2.58× (65 %) |
