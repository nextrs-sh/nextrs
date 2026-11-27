# engineering/performance/resource-efficiency/results/re23-cycles

## 256m memory, 1 vCPU

| Variant | Runs | OOM / failed runs | Failed requests | Contended runs | Heap limit MiB | Ready ms | Idle MiB (anon) | Idle PSS MiB | Peak MiB (anon) | After 10 s | After 30 s | After 60 s | After 120 s | After 300 s | Anon after 300 s | Heap used/total after 300 s | malloc free after 300 s |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| base | 1 | r1: OOM | 1 | 0 | – | 5189 | 33.9 (30.5) | 67.9 | 254.0 (246.0) | – | – | – | – | – | – | – / – | – |
| re2b-combo | 1 | 0 | 0 | 0 | 112 | 2205 | 24.0 (20.1) | 57.3 | 165.6 (151.9) | 89.9 | 90.8 | 90.6 | 90.4 | 89.8 | 84.1 | 49.8 / 51.4 | 30.2 |

| Variant | Phase | Offered/s | Achieved/s | Failed | p50 ms | p99 ms | p99.9 ms | CPU ms/ok | CPU cores | Throttled ms | Steal % | Peak MiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| base | cycle1-mixed | 30 | 30.0 | 0 | 6.1 | 276.0 | 744.4 | 15.64 | 0.46 | 2465 | 0.0 | 211.4 |
| re2b-combo | cycle1-mixed | 30 | 30.0 | 0 | 5.8 | 542.7 | 1325.1 | 14.74 | 0.44 | 2692 | 0.0 | 121.0 |
| base | cycle1-burst | closed | – | 1 | – | – | – | – | – | – | 0.0 | 254.0 |
| re2b-combo | cycle1-burst | closed | 104.6 | 0 | 80.8 | 1100.8 | 1300.5 | 9.70 | 0.99 | 10445 | 0.0 | 161.4 |
| re2b-combo | cycle2-mixed | 30 | 30.0 | 0 | 5.5 | 398.8 | 901.6 | 13.31 | 0.40 | 2371 | 0.0 | 121.3 |
| re2b-combo | cycle2-burst | closed | 109.2 | 0 | 78.7 | 986.6 | 1408.0 | 9.24 | 0.98 | 10720 | 0.0 | 162.6 |
| re2b-combo | cycle3-mixed | 30 | 30.0 | 0 | 5.4 | 266.8 | 584.2 | 13.76 | 0.41 | 1998 | 0.0 | 122.4 |
| re2b-combo | cycle3-burst | closed | 94.8 | 0 | 87.3 | 1210.4 | 1417.2 | 10.83 | 0.98 | 10246 | 0.0 | 165.6 |

## 512m memory, 1 vCPU

| Variant | Runs | OOM / failed runs | Failed requests | Contended runs | Heap limit MiB | Ready ms | Idle MiB (anon) | Idle PSS MiB | Peak MiB (anon) | After 10 s | After 30 s | After 60 s | After 120 s | After 300 s | Anon after 300 s | Heap used/total after 300 s | malloc free after 300 s |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| base | 1 | 0 | 0 | 0 | – | 2508 | 33.9 (30.3) | 67.4 | 294.3 (281.5) | 169.1 | 169.0 | 169.4 | 168.1 | 169.9 | 159.2 | 49.8 / 52.4 | 88.2 |
| re2b-combo | 1 | 0 | 0 | 0 | 268 | 2275 | 24.0 (20.4) | 57.6 | 179.8 (168.2) | 92.7 | 93.6 | 93.2 | 95.4 | 95.8 | 91.0 | 49.9 / 51.6 | 41.2 |

| Variant | Phase | Offered/s | Achieved/s | Failed | p50 ms | p99 ms | p99.9 ms | CPU ms/ok | CPU cores | Throttled ms | Steal % | Peak MiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| base | cycle1-mixed | 30 | 30.0 | 0 | 5.4 | 140.9 | 362.2 | 12.50 | 0.37 | 1148 | 0.0 | 231.8 |
| re2b-combo | cycle1-mixed | 30 | 30.0 | 0 | 5.4 | 64.7 | 102.8 | 11.22 | 0.33 | 546 | 0.0 | 125.2 |
| base | cycle1-burst | closed | 135.2 | 0 | 63.2 | 867.8 | 1045.5 | 7.46 | 0.99 | 9580 | 0.0 | 294.3 |
| re2b-combo | cycle1-burst | closed | 128.7 | 0 | 66.4 | 876.0 | 963.1 | 7.85 | 0.99 | 10468 | 0.0 | 174.6 |
| base | cycle2-mixed | 30 | 30.0 | 0 | 5.1 | 102.7 | 164.9 | 12.76 | 0.38 | 880 | 0.0 | 240.2 |
| re2b-combo | cycle2-mixed | 30 | 30.0 | 0 | 5.4 | 66.4 | 73.4 | 11.06 | 0.33 | 587 | 0.0 | 130.8 |
| base | cycle2-burst | closed | 117.7 | 0 | 73.5 | 1015.8 | 1216.5 | 8.61 | 0.98 | 8366 | 0.0 | 291.6 |
| re2b-combo | cycle2-burst | closed | 123.7 | 0 | 71.8 | 886.8 | 993.3 | 8.21 | 0.99 | 10341 | 0.0 | 179.8 |
| base | cycle3-mixed | 30 | 30.0 | 0 | 5.3 | 113.5 | 271.9 | 11.78 | 0.35 | 709 | 0.0 | 235.0 |
| re2b-combo | cycle3-mixed | 30 | 30.0 | 0 | 5.5 | 74.5 | 244.1 | 11.10 | 0.33 | 656 | 0.0 | 128.5 |
| base | cycle3-burst | closed | 130.1 | 0 | 63.4 | 925.2 | 1276.9 | 7.61 | 0.96 | 8265 | 0.0 | 290.7 |
| re2b-combo | cycle3-burst | closed | 125.3 | 0 | 67.8 | 879.6 | 961.5 | 8.09 | 0.99 | 9713 | 0.0 | 177.4 |

