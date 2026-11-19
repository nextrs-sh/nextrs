# V8 GC per workload — benchmarks/results/phase3c/pages-p99-gc-base

Medians of 3 rounds (valid rows only). Closed loop, 32 connections, 15 s windows. `--trace-gc-nvp` on in every configuration.

| Workload | config | rps | cpu_ms_per_req | p99_ms | young_alloc_kib_per_req | minor_gcs_per_1k_req | mean_minor_pause_ms | major_gcs_per_1k_req | minor_pause_ms_per_req | gc_pause_share_of_cpu | anon_end_mib | anon_per_isolate_mib | heap after GC MiB (per isolate) | committed MiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---|
| pages/pages-ssr-proxied | default-i1 | 528 | 1.741 | 7.687 | 568 | 22.47 | 2.159 | 5.554 | 0.049 | 3.2 | 109 | 109 | 13.1 | 78.9 |
