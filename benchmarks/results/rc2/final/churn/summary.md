Large static response: `/_next/static/chunks/39_j1bdzreq0m.js` (229288 bytes).

Medians over rounds. `base`: NextRS before this branch; `cand`: this branch; `next`: next start. `-gzip` requests send `Accept-Encoding: gzip`, `-identity` ones `identity`. CPU: server process tree, over the measured window (4 CPUs available). Bytes: body bytes per response as received.

### static-mix

| config | req/s | p50 ms | p95 ms | p99 ms | TTFB p50 ms | TTFB p99 ms | CPU (cores) | CPU µs/req | bytes/resp | encoding | peak RSS MiB | failed |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---|---:|---:|
| base-gzip | 3238 | 10.17 | 15.17 | 17.66 | 10.16 | 17.65 | 3.91 | 1206.4 | 25275 | identity | 66.1 | 0 |
| cand-gzip | 46209 | 0.64 | 1.18 | 1.80 | 0.64 | 1.79 | 3.44 | 74.3 | 25273 | identity | 77.8 | 0 |
| cand-nocache-gzip | 3227 | 10.18 | 15.26 | 17.77 | 10.17 | 17.77 | 3.92 | 1215.9 | 25278 | identity | 69.4 | 0 |
| cand-tiny-gzip | 2952 | 10.51 | 18.18 | 22.85 | 10.50 | 22.83 | 3.63 | 1231.3 | 25267 | identity | 84.1 | 0 |

