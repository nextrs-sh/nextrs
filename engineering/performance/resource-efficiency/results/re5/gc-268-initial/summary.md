# V8 GC per workload — engineering/performance/resource-efficiency/results/re5/gc-268-initial

Medians of 2 rounds (valid rows only). Closed loop, 32 connections, 10 s windows. `--trace-gc-nvp` on in every configuration.

| Workload | config | rps | cpu_ms_per_req | p99_ms | young_alloc_kib_per_req | minor_gcs_per_1k_req | mean_minor_pause_ms | major_gcs_per_1k_req | minor_pause_ms_per_req | gc_pause_share_of_cpu | anon_end_mib | anon_per_isolate_mib | heap after GC MiB (per isolate) | committed MiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---|
| app/rsc | default-i1 | 1293 | 1.049 | 35.375 | 287 | 17.09 | 2.234 | 3.441 | 0.038 | 4.9 | 96.05 | 96.05 | 23.1 | 63.7 |
| app/rsc | semi16-i1 | 1320 | 1.004 | 34.639 | 286 | 14.765 | 1.821 | 3.185 | 0.027 | 3.95 | 104 | 104 | 23.1 | 66 |
| app/rsc | semi8-i1 | 1304 | 1.042 | 35.167 | 286 | 17.1 | 2.206 | 3.45 | 0.038 | 4.9 | 94.7 | 94.7 | 23.1 | 63.6 |
| app/ssr | default-i1 | 720 | 1.884 | 65.055 | 533 | 30.815 | 1.774 | 6.941 | 0.055 | 4.15 | 91.15 | 91.15 | 20 | 63.2 |
| app/ssr | semi16-i1 | 730 | 1.896 | 56.927 | 533 | 31.18 | 1.926 | 7.743 | 0.06 | 4.4 | 94.1 | 94.1 | 20.5 | 60.2 |
| app/ssr | semi8-i1 | 723 | 1.876 | 64.431 | 533 | 30.63 | 1.768 | 6.983 | 0.054 | 4.1 | 92.7 | 92.7 | 19.9 | 56.5 |
