Large static response: `/_next/static/chunks/39_j1bdzreq0m.js` (229288 bytes).

Medians over rounds. `base`: NextRS before this branch; `cand`: this branch; `next`: next start. `-gzip` requests send `Accept-Encoding: gzip`, `-identity` ones `identity`. CPU: server process tree, over the measured window (4 CPUs available). Bytes: body bytes per response as received.

### prerendered

| config | req/s | p50 ms | p95 ms | p99 ms | TTFB p50 ms | TTFB p99 ms | CPU (cores) | CPU µs/req | bytes/resp | encoding | peak RSS MiB | failed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|
| base-identity | 116447 | 0.26 | 0.47 | 0.69 | 0.26 | 0.69 | 2.35 | 20.2 | 6070 | identity | 61.3 | 0 |
| cand-identity | 119877 | 0.26 | 0.41 | 0.63 | 0.26 | 0.63 | 2.52 | 20.8 | 6070 | identity | 65.8 | 0 |
| next-identity | 2901 | 10.32 | 14.35 | 20.99 | 10.32 | 20.99 | 1.09 | 377.6 | 6070 | identity | 372.9 | 0 |
| base-gzip | 21998 | 1.45 | 2.28 | 2.64 | 1.45 | 2.59 | 3.81 | 173.4 | 2012 | gzip | 62.5 | 0 |
| cand-gzip | 133161 | 0.23 | 0.37 | 0.54 | 0.23 | 0.54 | 2.74 | 20.6 | 2006 | gzip | 64.8 | 0 |
| next-gzip | 2541 | 11.64 | 16.99 | 22.59 | 10.23 | 15.35 | 1.42 | 560.3 | 1998 | gzip | 323.5 | 0 |

### ssr

| config | req/s | p50 ms | p95 ms | p99 ms | TTFB p50 ms | TTFB p99 ms | CPU (cores) | CPU µs/req | bytes/resp | encoding | peak RSS MiB | failed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|
| base-identity | 752 | 42.05 | 49.60 | 64.86 | 42.05 | 64.86 | 1.28 | 1727.4 | 6038 | identity | 193.3 | 0 |
| cand-identity | 739 | 42.91 | 48.32 | 63.33 | 42.91 | 63.33 | 1.30 | 1766.8 | 6038 | identity | 188.0 | 0 |
| next-identity | 603 | 50.98 | 59.13 | 72.83 | 50.98 | 72.77 | 1.19 | 1966.8 | 6038 | identity | 443.8 | 0 |
| base-gzip | 774 | 40.73 | 48.38 | 61.15 | 40.73 | 61.15 | 1.39 | 1804.8 | 1893 | gzip | 191.6 | 0 |
| cand-gzip | 746 | 42.34 | 48.51 | 62.59 | 42.30 | 62.56 | 1.40 | 1883.0 | 1893 | gzip | 192.6 | 0 |
| next-gzip | 569 | 54.81 | 62.62 | 79.10 | 39.33 | 57.15 | 1.25 | 2216.2 | 1989 | gzip | 449.0 | 0 |

### rsc

| config | req/s | p50 ms | p95 ms | p99 ms | TTFB p50 ms | TTFB p99 ms | CPU (cores) | CPU µs/req | bytes/resp | encoding | peak RSS MiB | failed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|
| base-identity | 1347 | 22.27 | 32.30 | 35.52 | 21.47 | 34.59 | 1.30 | 968.7 | 3729 | identity | 191.2 | 0 |
| cand-identity | 1337 | 22.40 | 32.64 | 35.55 | 21.63 | 34.46 | 1.30 | 975.6 | 3729 | identity | 189.9 | 0 |
| next-identity | 945 | 32.27 | 40.19 | 47.23 | 32.26 | 47.23 | 1.14 | 1209.3 | 3729 | identity | 471.8 | 0 |
| base-gzip | 1310 | 23.54 | 30.24 | 38.40 | 22.80 | 37.44 | 1.44 | 1102.5 | 1403 | gzip | 197.3 | 0 |
| cand-gzip | 1291 | 23.63 | 33.38 | 38.94 | 22.85 | 37.98 | 1.44 | 1120.5 | 1403 | gzip | 199.7 | 0 |
| next-gzip | 807 | 38.56 | 46.24 | 54.43 | 7.66 | 27.21 | 1.26 | 1550.3 | 1675 | gzip | 451.6 | 0 |

### json-api

| config | req/s | p50 ms | p95 ms | p99 ms | TTFB p50 ms | TTFB p99 ms | CPU (cores) | CPU µs/req | bytes/resp | encoding | peak RSS MiB | failed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|
| base-identity | 4326 | 6.92 | 11.83 | 13.75 | 6.12 | 12.81 | 1.50 | 346.5 | 42 | identity | 203.5 | 0 |
| cand-identity | 4334 | 6.90 | 11.78 | 13.68 | 6.11 | 12.77 | 1.51 | 349.7 | 42 | identity | 203.9 | 0 |
| next-identity | 2246 | 13.47 | 18.14 | 23.04 | 13.47 | 23.04 | 1.12 | 494.6 | 42 | identity | 505.1 | 0 |
| base-gzip | 4216 | 7.10 | 11.97 | 13.85 | 6.32 | 12.97 | 1.71 | 401.7 | 68 | gzip | 208.6 | 0 |
| cand-gzip | 4174 | 7.17 | 12.12 | 14.13 | 6.38 | 13.19 | 1.65 | 403.9 | 68 | gzip | 211.8 | 0 |
| next-gzip | 1944 | 15.54 | 21.14 | 26.59 | 9.37 | 15.84 | 1.19 | 615.3 | 62 | gzip | 457.5 | 0 |

### large-static

| config | req/s | p50 ms | p95 ms | p99 ms | TTFB p50 ms | TTFB p99 ms | CPU (cores) | CPU µs/req | bytes/resp | encoding | peak RSS MiB | failed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|
| base-identity | 10580 | 2.87 | 5.08 | 6.75 | 2.79 | 6.62 | 2.64 | 249.5 | 229288 | identity | 198.6 | 0 |
| cand-identity | 11408 | 2.68 | 4.59 | 5.81 | 2.60 | 5.69 | 2.90 | 255.9 | 229288 | identity | 205.4 | 0 |
| next-identity | 3705 | 7.60 | 14.34 | 16.50 | 4.69 | 11.12 | 1.64 | 443.2 | 229288 | identity | 555.1 | 0 |
| base-gzip | 984 | 33.05 | 48.67 | 56.00 | 33.05 | 55.97 | 3.94 | 4026.0 | 72549 | gzip | 204.2 | 0 |
| cand-gzip | 24824 | 1.21 | 2.23 | 3.14 | 1.20 | 3.10 | 3.19 | 121.0 | 72542 | gzip | 206.5 | 0 |
| next-gzip | 619 | 49.31 | 62.62 | 65.18 | 16.18 | 30.91 | 3.93 | 6375.8 | 71752 | gzip | 467.3 | 0 |

### streaming-64

| config | req/s | p50 ms | p95 ms | p99 ms | TTFB p50 ms | TTFB p99 ms | CPU (cores) | CPU µs/req | bytes/resp | encoding | peak RSS MiB | failed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|
| base-identity | 609 | 104.13 | 122.94 | 136.70 | 77.25 | 113.92 | 1.30 | 2161.0 | 7150 | identity | 219.8 | 0 |
| cand-identity | 611 | 103.68 | 125.18 | 140.41 | 72.83 | 121.02 | 1.31 | 2160.7 | 7150 | identity | 223.8 | 0 |
| next-identity | 607 | 103.55 | 128.32 | 143.23 | 78.46 | 132.86 | 1.15 | 1909.2 | 7150 | identity | 520.2 | 0 |
| base-gzip | 616 | 102.14 | 123.65 | 137.22 | 76.42 | 114.24 | 1.42 | 2329.2 | 2556 | gzip | 247.0 | 0 |
| cand-gzip | 608 | 104.13 | 124.03 | 143.74 | 74.50 | 120.51 | 1.41 | 2353.6 | 2556 | gzip | 251.4 | 0 |
| next-gzip | 556 | 114.30 | 131.97 | 143.74 | 48.45 | 87.10 | 1.28 | 2325.4 | 2857 | gzip | 477.9 | 0 |

### static-mix

| config | req/s | p50 ms | p95 ms | p99 ms | TTFB p50 ms | TTFB p99 ms | CPU (cores) | CPU µs/req | bytes/resp | encoding | peak RSS MiB | failed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|
| base-identity | 47623 | 0.63 | 1.15 | 1.78 | 0.62 | 1.72 | 3.26 | 67.9 | 28427 | identity | 218.7 | 0 |
| cand-identity | 47019 | 0.64 | 1.14 | 1.73 | 0.63 | 1.68 | 3.29 | 70.0 | 28427 | identity | 223.0 | 0 |
| next-identity | 7617 | 3.77 | 6.38 | 10.21 | 3.75 | 9.79 | 1.23 | 161.3 | 28427 | identity | 522.5 | 0 |
| base-gzip | 7374 | 4.20 | 7.68 | 9.38 | 4.20 | 9.36 | 3.82 | 518.3 | 9044 | identity | 238.4 | 0 |
| cand-gzip | 53504 | 0.55 | 1.04 | 1.68 | 0.54 | 1.67 | 3.37 | 63.1 | 9040 | identity | 246.5 | 0 |
| next-gzip | 4481 | 6.05 | 16.78 | 21.04 | 4.95 | 12.89 | 3.65 | 814.6 | 8966 | identity | 478.9 | 0 |

### range

| config | req/s | p50 ms | p95 ms | p99 ms | TTFB p50 ms | TTFB p99 ms | CPU (cores) | CPU µs/req | bytes/resp | encoding | peak RSS MiB | failed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|
| base-identity | 11444 | 2.66 | 4.66 | 5.92 | 2.59 | 5.79 | 2.80 | 247.8 | 229288 | identity | 113.2 | 348883 |
| cand-identity | 57391 | 0.52 | 0.94 | 1.47 | 0.52 | 1.47 | 3.37 | 58.9 | 16384 | identity | 116.9 | 0 |
| next-identity | 8239 | 3.65 | 4.32 | 9.68 | 3.64 | 9.68 | 1.23 | 149.8 | 16384 | identity | 522.2 | 0 |
| base-gzip | 986 | 32.99 | 48.22 | 56.06 | 32.96 | 56.03 | 3.96 | 4027.8 | 72549 | gzip | 120.0 | 29589 |
| cand-gzip | 57001 | 0.52 | 0.95 | 1.53 | 0.52 | 1.53 | 3.37 | 59.3 | 16384 | identity | 122.7 | 0 |
| next-gzip | 5756 | 5.24 | 7.78 | 12.86 | 4.45 | 11.81 | 2.60 | 451.8 | 5504 | gzip | 478.5 | 0 |

### conditional

| config | req/s | p50 ms | p95 ms | p99 ms | TTFB p50 ms | TTFB p99 ms | CPU (cores) | CPU µs/req | bytes/resp | encoding | peak RSS MiB | failed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|
| base-identity | 93870 | 0.32 | 0.56 | 1.00 | 0.32 | 1.00 | 3.14 | 33.2 | 0 | identity | 112.0 | 0 |
| cand-identity | 93663 | 0.33 | 0.56 | 1.01 | 0.33 | 1.01 | 3.22 | 34.5 | 0 | identity | 116.9 | 0 |
| next-identity | 4517 | 7.46 | 11.35 | 13.38 | 7.46 | 13.38 | 1.06 | 235.3 | 0 | identity | 524.8 | 0 |
| base-gzip | 95464 | 0.32 | 0.55 | 0.96 | 0.32 | 0.96 | 3.19 | 33.4 | 0 | gzip | 119.2 | 0 |
| cand-gzip | 91733 | 0.34 | 0.57 | 0.97 | 0.34 | 0.97 | 3.20 | 34.9 | 0 | gzip | 122.7 | 0 |
| next-gzip | 4619 | 7.36 | 11.10 | 13.03 | 7.36 | 13.03 | 1.07 | 230.3 | 0 | gzip | 505.1 | 0 |

