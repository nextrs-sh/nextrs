# V8 GC per workload — benchmarks/results/phase3c/pages-p99-gc-p3a

Medians of 3 rounds (valid rows only). Closed loop, 32 connections, 15 s windows. `--trace-gc-nvp` on in every configuration.

| Workload | config | rps | cpu_ms_per_req | p99_ms | young_alloc_kib_per_req | minor_gcs_per_1k_req | mean_minor_pause_ms | major_gcs_per_1k_req | minor_pause_ms_per_req | gc_pause_share_of_cpu | anon_end_mib | anon_per_isolate_mib | heap after GC MiB (per isolate) | committed MiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---|
| pages/pages-ssr-proxied | default-i1 | 528 | 1.55 | 7.943 | 341 | 14.14 | 3.146 | 4.67 | 0.044 | 3.3 | 114 | 114 | 13.9 | 80.1 |
