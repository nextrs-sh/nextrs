Large static response: `/_next/static/chunks/01t2i9pe5vz2-.js` (229318 bytes).

Medians over rounds. `base`: NextRS before this branch; `cand`: this branch; `next`: next start. `-gzip` requests send `Accept-Encoding: gzip`, `-identity` ones `identity`. CPU: server process tree, over the measured window (4 CPUs available). Bytes: body bytes per response as received.

### ppr-streaming

| config | req/s | p50 ms | p95 ms | p99 ms | TTFB p50 ms | TTFB p99 ms | CPU (cores) | CPU µs/req | bytes/resp | encoding | peak RSS MiB | failed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|
| cand-identity | 575 | 54.66 | 66.62 | 74.94 | 11.57 | 23.57 | 1.31 | 2285.6 | 6305 | identity | 206.0 | 0 |
| base-gzip | 568 | 55.30 | 68.35 | 74.75 | 12.28 | 23.05 | 1.39 | 2462.5 | 2365 | gzip | 223.6 | 0 |
| cand-gzip | 567 | 55.39 | 67.52 | 76.67 | 12.22 | 19.26 | 1.37 | 2426.7 | 2365 | gzip | 235.7 | 0 |
| next-gzip | 394 | 81.73 | 95.10 | 103.42 | 23.28 | 53.73 | 1.28 | 3263.9 | 2610 | gzip | 439.6 | 0 |

### cc-prerendered

| config | req/s | p50 ms | p95 ms | p99 ms | TTFB p50 ms | TTFB p99 ms | CPU (cores) | CPU µs/req | bytes/resp | encoding | peak RSS MiB | failed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|
| cand-identity | 608 | 51.65 | 68.93 | 70.78 | 11.47 | 18.83 | 1.26 | 2105.7 | 6285 | identity | 212.1 | 0 |
| base-gzip | 595 | 52.90 | 66.62 | 70.27 | 11.38 | 18.50 | 1.35 | 2273.2 | 2358 | gzip | 232.4 | 0 |
| cand-gzip | 603 | 52.19 | 64.48 | 71.81 | 11.27 | 22.06 | 1.36 | 2255.6 | 2357 | gzip | 240.8 | 0 |
| next-gzip | 416 | 77.44 | 89.41 | 96.51 | 23.82 | 47.17 | 1.24 | 2996.6 | 2602 | gzip | 449.0 | 0 |

