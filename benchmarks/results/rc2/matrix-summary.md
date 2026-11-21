Large static response: `/_next/static/chunks/39_j1bdzreq0m.js` (229288 bytes).

Medians over rounds. `base`: NextRS before this branch; `cand`: this branch; `next`: next start. `-gzip` requests send `Accept-Encoding: gzip`, `-identity` ones `identity`. CPU: server process tree, over the measured window (4 CPUs available). Bytes: body bytes per response as received.

### prerendered

| config | req/s | p50 ms | p95 ms | p99 ms | TTFB p50 ms | TTFB p99 ms | CPU (cores) | CPU µs/req | bytes/resp | encoding | peak RSS MiB | failed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|
| base-identity | 120412 | 0.26 | 0.40 | 0.62 | 0.26 | 0.62 | 2.31 | 19.1 | 6070 | identity | 60.8 | 0 |
| cand-identity | 121686 | 0.25 | 0.39 | 0.62 | 0.25 | 0.62 | 2.44 | 20.4 | 6070 | identity | 67.3 | 0 |
| next-identity | 2923 | 10.23 | 14.16 | 20.64 | 10.23 | 20.64 | 1.09 | 371.9 | 6070 | identity | 371.8 | 0 |
| base-gzip | 124288 | 0.25 | 0.38 | 0.60 | 0.25 | 0.60 | 2.38 | 19.0 | 6070 | identity | 60.8 | 0 |
| cand-gzip | 25310 | 1.26 | 2.00 | 2.37 | 1.25 | 2.31 | 3.80 | 149.7 | 2014 | gzip | 67.1 | 0 |
| next-gzip | 2549 | 11.63 | 17.52 | 21.25 | 9.94 | 15.80 | 1.41 | 555.4 | 2000 | gzip | 325.4 | 0 |

### ssr

| config | req/s | p50 ms | p95 ms | p99 ms | TTFB p50 ms | TTFB p99 ms | CPU (cores) | CPU µs/req | bytes/resp | encoding | peak RSS MiB | failed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|
| base-identity | 746 | 42.40 | 48.26 | 64.96 | 42.40 | 64.93 | 1.28 | 1732.8 | 6038 | identity | 191.9 | 0 |
| cand-identity | 765 | 41.47 | 47.39 | 65.60 | 41.47 | 65.53 | 1.27 | 1668.4 | 6038 | identity | 195.8 | 0 |
| next-identity | 617 | 49.57 | 60.35 | 72.32 | 49.53 | 72.32 | 1.19 | 1951.4 | 6038 | identity | 428.2 | 0 |
| base-gzip | 765 | 41.57 | 46.62 | 64.32 | 41.57 | 64.32 | 1.27 | 1673.6 | 6038 | identity | 188.0 | 0 |
| cand-gzip | 749 | 42.21 | 48.06 | 64.00 | 42.17 | 64.00 | 1.37 | 1834.9 | 1893 | gzip | 190.3 | 0 |
| next-gzip | 553 | 55.90 | 64.48 | 79.94 | 34.94 | 60.67 | 1.26 | 2276.7 | 1989 | gzip | 435.4 | 0 |

### rsc

| config | req/s | p50 ms | p95 ms | p99 ms | TTFB p50 ms | TTFB p99 ms | CPU (cores) | CPU µs/req | bytes/resp | encoding | peak RSS MiB | failed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|
| base-identity | 1353 | 22.29 | 30.57 | 35.04 | 21.50 | 33.92 | 1.28 | 957.4 | 3729 | identity | 192.8 | 0 |
| cand-identity | 1405 | 22.14 | 28.82 | 34.69 | 21.18 | 33.28 | 1.25 | 894.0 | 3729 | identity | 198.4 | 0 |
| next-identity | 941 | 32.08 | 40.99 | 56.38 | 32.08 | 56.38 | 1.14 | 1216.3 | 3729 | identity | 470.5 | 0 |
| base-gzip | 1362 | 22.02 | 29.25 | 34.56 | 21.23 | 33.50 | 1.30 | 956.3 | 3729 | identity | 187.0 | 0 |
| cand-gzip | 1299 | 23.76 | 33.47 | 39.77 | 22.98 | 38.69 | 1.48 | 1145.4 | 1404 | gzip | 198.9 | 0 |
| next-gzip | 794 | 38.78 | 48.09 | 55.62 | 7.78 | 22.57 | 1.26 | 1577.9 | 1676 | gzip | 438.7 | 0 |

### json-api

| config | req/s | p50 ms | p95 ms | p99 ms | TTFB p50 ms | TTFB p99 ms | CPU (cores) | CPU µs/req | bytes/resp | encoding | peak RSS MiB | failed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|
| base-identity | 4464 | 6.72 | 11.41 | 13.08 | 5.94 | 12.25 | 1.50 | 337.4 | 42 | identity | 202.8 | 0 |
| cand-identity | 4474 | 6.71 | 11.37 | 13.15 | 5.93 | 12.33 | 1.51 | 335.7 | 42 | identity | 203.1 | 0 |
| next-identity | 2234 | 13.18 | 19.38 | 25.81 | 13.18 | 25.81 | 1.11 | 495.1 | 42 | identity | 505.8 | 0 |
| base-gzip | 4477 | 6.66 | 11.47 | 13.34 | 5.88 | 12.48 | 1.51 | 336.5 | 42 | identity | 204.5 | 0 |
| cand-gzip | 4199 | 7.14 | 12.12 | 13.94 | 6.37 | 13.07 | 1.68 | 411.0 | 68 | gzip | 213.2 | 0 |
| next-gzip | 1951 | 15.46 | 21.41 | 26.38 | 8.91 | 16.28 | 1.19 | 609.8 | 62 | gzip | 451.3 | 0 |

### large-static

| config | req/s | p50 ms | p95 ms | p99 ms | TTFB p50 ms | TTFB p99 ms | CPU (cores) | CPU µs/req | bytes/resp | encoding | peak RSS MiB | failed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|
| base-identity | 11683 | 2.61 | 4.47 | 5.74 | 2.54 | 5.61 | 2.88 | 245.2 | 229288 | identity | 201.2 | 0 |
| cand-identity | 11594 | 2.64 | 4.51 | 5.69 | 2.57 | 5.58 | 2.83 | 249.2 | 229288 | identity | 207.5 | 0 |
| next-identity | 3760 | 7.51 | 14.12 | 16.61 | 4.65 | 10.82 | 1.65 | 439.9 | 229288 | identity | 556.9 | 0 |
| base-gzip | 11633 | 2.63 | 4.48 | 5.70 | 2.56 | 5.59 | 2.85 | 246.5 | 229288 | identity | 202.5 | 0 |
| cand-gzip | 980 | 33.22 | 48.51 | 56.58 | 33.22 | 56.54 | 3.93 | 4030.0 | 72549 | gzip | 208.3 | 0 |
| next-gzip | 614 | 49.53 | 63.07 | 65.28 | 16.36 | 31.54 | 3.93 | 6422.9 | 71752 | gzip | 461.4 | 0 |

### streaming-64

| config | req/s | p50 ms | p95 ms | p99 ms | TTFB p50 ms | TTFB p99 ms | CPU (cores) | CPU µs/req | bytes/resp | encoding | peak RSS MiB | failed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|
| base-identity | 614 | 103.30 | 122.17 | 137.60 | 75.84 | 115.26 | 1.30 | 2135.6 | 7150 | identity | 209.7 | 0 |
| cand-identity | 619 | 102.21 | 122.17 | 133.76 | 73.86 | 112.64 | 1.30 | 2122.4 | 7150 | identity | 212.9 | 0 |
| next-identity | 621 | 102.27 | 123.45 | 140.16 | 75.58 | 116.42 | 1.16 | 1889.8 | 7150 | identity | 545.9 | 0 |
| base-gzip | 610 | 103.10 | 124.86 | 141.06 | 78.21 | 113.86 | 1.30 | 2141.9 | 7150 | identity | 218.1 | 0 |
| cand-gzip | 618 | 102.91 | 123.01 | 135.94 | 75.14 | 112.64 | 1.41 | 2315.9 | 2558 | gzip | 249.8 | 0 |
| next-gzip | 555 | 114.17 | 134.53 | 149.12 | 48.80 | 93.57 | 1.28 | 2318.0 | 2858 | gzip | 479.5 | 0 |

