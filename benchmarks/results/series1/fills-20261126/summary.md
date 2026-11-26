# Summary of benchmarks/results/series1/fills-20261126

Median across rounds, with (min–max). n = rounds.

## same-asset-burst-32

| runtime | n | rps | p50 | p95 | p99 | max | ttfb_p50 | cpu_ms_per_req | req_per_cpu_s | rss_mib | failed | vm_busy |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| node | 2 | 136 (129–143) | 323 (304–341) | 527 (419–636) | 558 (433–684) | 558 (433–684) | 110 (83.3–136) | 21.9 (21.0–22.8) | 46.0 (44.0–48.0) | 157 (133–181) | 0 (0–0) | 0.197 (0.191–0.202) |
| optA | 4 | 1405 (1028–1615) | 29.7 (27.3–33.2) | 39.0 (37.4–51.7) | 42.2 (38.1–52.8) | 42.2 (38.1–52.8) | 29.1 (26.3–31.6) | 3.917 (2.55–4.946) | 256 (202–392) | 74.3 (73.3–75.4) | 0 (0–0) | 0.146 (0.1–0.197) |

| runtime (NextRS metrics) | via_executor | exec_ms_per_req | cache_value_kib_per_req | cache_reused_per_req | stream_frames_per_req | stream_gzip_ms_per_req | heap_used_mib |
|---|---:|---:|---:|---:|---:|---:|---:|
| optA | 0 (0–0) | 0 (0–0) | 0 (0–0) | 0 (0–0) | 0 (0–0) | 0 (0–0) | 13.1 (13.0–13.6) |

## all-assets-burst-32

| runtime | n | rps | p50 | p95 | p99 | max | ttfb_p50 | cpu_ms_per_req | req_per_cpu_s | rss_mib | failed | vm_busy |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| node | 2 | 443 (318–569) | 98.8 (78.8–119) | 154 (117–190) | 187 (153–220) | 187 (153–220) | 72.8 (55.7–89.9) | 12.5 (9.649–15.3) | 84.5 (65.0–104) | 158 (134–182) | 0 (0–0) | 0.209 (0.177–0.241) |
| optA | 4 | 2009 (884–2528) | 21.3 (16.3–25.6) | 32.6 (27.7–36.1) | 35.3 (27.9–38.0) | 35.3 (27.9–38.0) | 21.3 (16.3–25.6) | 4.288 (2.948–9.739) | 238 (103–339) | 76.5 (74.7–78.9) | 0 (0–0) | 0.155 (0.128–0.185) |

| runtime (NextRS metrics) | via_executor | exec_ms_per_req | cache_value_kib_per_req | cache_reused_per_req | stream_frames_per_req | stream_gzip_ms_per_req | heap_used_mib |
|---|---:|---:|---:|---:|---:|---:|---:|
| optA | 0 (0–0) | 0 (0–0) | 0 (0–0) | 0 (0–0) | 0 (0–0) | 0 (0–0) | 13.6 (13.6–13.6) |

## static-open-400

| runtime | n | rps | p50 | p95 | p99 | max | ttfb_p50 | cpu_ms_per_req | req_per_cpu_s | rss_mib | failed | vm_busy |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| node | 2 | 397 (394–400) | 507 (14.4–1000) | 958 (135–1782) | 1089 (280–1898) | 1330 (535–2126) | 51.2 (9.759–92.7) | 4.317 (3.907–4.727) | 234 (212–256) | 197 (162–232) | 0 (0–0) | 0.236 (0.216–0.257) |
| optA | 4 | 400 (400–400) | 1.627 (1.448–1.811) | 8.923 (3.433–17.4) | 54.8 (25.6–94.6) | 194 (93.5–227) | 1.192 (1.044–1.381) | 1.134 (0.994–1.253) | 888 (798–1006) | 76.7 (75.4–79.2) | 0 (0–0) | 0.072 (0.062–0.086) |

| runtime (NextRS metrics) | via_executor | exec_ms_per_req | cache_value_kib_per_req | cache_reused_per_req | stream_frames_per_req | stream_gzip_ms_per_req | heap_used_mib |
|---|---:|---:|---:|---:|---:|---:|---:|
| optA | 6 (6–6) | 0.003 (0.001–0.004) | 0 (0–0) | 0 (0–0) | 0 (0–0) | 0 (0–0) | 14.8 (14.7–14.8) |

## Process and memory per run

| runtime | isolates | js | n | ready_ms | idle_cpu_millicores | idle_mem_mib | idle_rss_mib | idle_rss_anon_mib | idle_threads | peak_mem_mib | peak_rss_mib | recovered_mem_mib | recovered_rss_mib | recovered_rss_anon_mib | idle_heap_used_mib | recovered_heap_used_mib |
|---|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| node | 1 | default | 2 | n/a | n/a | n/a | n/a | n/a | n/a | 195 (168–221) | 200 (167–232) | n/a | n/a | n/a | n/a | n/a |
| optA | 1 | default | 4 | n/a | n/a | n/a | n/a | n/a | n/a | 53.8 (51.4–56.2) | 78.2 (76.8–79.5) | n/a | n/a | n/a | n/a | n/a |
