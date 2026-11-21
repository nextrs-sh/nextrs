Large static response: `/_next/static/chunks/39_j1bdzreq0m.js` (229288 bytes).

Medians over rounds. `base`: NextRS before this branch; `cand`: this branch; `next`: next start. `-gzip` requests send `Accept-Encoding: gzip`, `-identity` ones `identity`. CPU: server process tree, over the measured window (4 CPUs available). Bytes: body bytes per response as received.

### prerendered

| config | req/s | p50 ms | p95 ms | p99 ms | TTFB p50 ms | TTFB p99 ms | CPU (cores) | CPU µs/req | bytes/resp | encoding | peak RSS MiB | failed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|
| base-gzip | 25589 | 1.25 | 1.97 | 2.30 | 1.24 | 2.25 | 3.81 | 148.8 | 2012 | gzip | 62.5 | 0 |
| cand-gzip | 138413 | 0.22 | 0.35 | 0.50 | 0.22 | 0.50 | 2.91 | 20.9 | 2006 | gzip | 65.6 | 0 |
| cand-nocache-gzip | 21768 | 1.46 | 2.32 | 2.73 | 1.46 | 2.65 | 3.81 | 175.2 | 2012 | gzip | 66.0 | 0 |
| cand-tiny-gzip | 134697 | 0.23 | 0.35 | 0.50 | 0.23 | 0.50 | 2.85 | 21.0 | 2006 | gzip | 65.3 | 0 |

### large-static

| config | req/s | p50 ms | p95 ms | p99 ms | TTFB p50 ms | TTFB p99 ms | CPU (cores) | CPU µs/req | bytes/resp | encoding | peak RSS MiB | failed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|
| base-gzip | 973 | 33.44 | 48.64 | 56.73 | 33.41 | 56.70 | 3.95 | 4079.1 | 72549 | gzip | 68.3 | 0 |
| cand-gzip | 22454 | 1.34 | 2.46 | 3.26 | 1.34 | 3.25 | 3.33 | 147.3 | 72542 | gzip | 73.0 | 0 |
| cand-nocache-gzip | 967 | 33.70 | 48.51 | 56.70 | 33.70 | 56.70 | 3.95 | 4105.9 | 72549 | gzip | 72.0 | 0 |
| cand-tiny-gzip | 22932 | 1.32 | 2.39 | 3.23 | 1.31 | 3.21 | 3.30 | 145.5 | 72542 | gzip | 73.7 | 0 |

### static-mix

| config | req/s | p50 ms | p95 ms | p99 ms | TTFB p50 ms | TTFB p99 ms | CPU (cores) | CPU µs/req | bytes/resp | encoding | peak RSS MiB | failed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|
| base-gzip | 6918 | 4.51 | 8.07 | 9.77 | 4.51 | 9.73 | 3.85 | 557.5 | 9044 | identity | 68.1 | 0 |
| cand-gzip | 52735 | 0.56 | 1.04 | 1.58 | 0.56 | 1.57 | 3.38 | 64.4 | 9040 | identity | 72.4 | 0 |
| cand-nocache-gzip | 6722 | 4.61 | 8.31 | 10.05 | 4.60 | 10.02 | 3.82 | 569.4 | 9044 | identity | 71.4 | 0 |
| cand-tiny-gzip | 53786 | 0.55 | 1.01 | 1.48 | 0.55 | 1.47 | 3.43 | 63.9 | 9040 | identity | 72.1 | 0 |

