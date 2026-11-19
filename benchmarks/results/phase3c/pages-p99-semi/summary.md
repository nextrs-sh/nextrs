# V8 GC per workload — benchmarks/results/phase3c/pages-p99-semi

Medians of 3 rounds (valid rows only). Closed loop, 32 connections, 15 s windows. `--trace-gc-nvp` on in every configuration.

| Workload | config | rps | cpu_ms_per_req | p99_ms | young_alloc_kib_per_req | minor_gcs_per_1k_req | mean_minor_pause_ms | major_gcs_per_1k_req | minor_pause_ms_per_req | gc_pause_share_of_cpu | anon_end_mib | anon_per_isolate_mib | heap after GC MiB (per isolate) | committed MiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---|
| pages/pages-ssr-proxied | default-i1 | 528 | 1.564 | 7.927 | 343 | 14.39 | 3.108 | 4.797 | 0.045 | 3.2 | 111 | 111 | 13.8 | 79.6 |
| pages/pages-ssr-proxied | semi2-i1 | 528 | 1.684 | 5.235 | 346 | 238 | 0.362 | 8.078 | 0.086 | 5.6 | 56.6 | 56.6 | 13.9 | 29.6 |
| pages/pages-ssr-proxied | semi4-i1 | 528 | 1.652 | 5.339 | 346 | 119 | 0.542 | 7.573 | 0.064 | 4.3 | 66.6 | 66.6 | 13.7 | 32.8 |
| pages/pages-ssr-proxied | semi8-i1 | 528 | 1.629 | 5.475 | 346 | 58.44 | 0.924 | 7.195 | 0.054 | 3.8 | 73 | 73 | 13.6 | 39.3 |
