# engineering/performance/resource-efficiency/results/re23-gctrace

## 256m memory, 1 vCPU

| Variant | Runs | OOM / failed runs | Failed requests | Contended runs | Heap limit MiB | Ready ms | Idle MiB (anon) | Idle PSS MiB | Peak MiB (anon) | After 10 s | After 30 s | After 60 s | After 120 s | Anon after 120 s | Heap used/total after 120 s | malloc free after 120 s |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| base-gc | 1 | r1: OOM | 1 | 0 | – | 3544 | 35.3 (29.0) | 67.2 | 255.9 (244.0) | – | – | – | – | – | – / – | – |
| re2b-combo-gc | 1 | 0 | 0 | 0 | 112 | 2589 | 22.3 (19.4) | 56.7 | 166.2 (158.3) | 82.6 | 83.0 | 86.0 | 85.8 | 77.2 | 49.1 / 51.2 | 35.6 |
| re2b-heap-gc | 1 | 0 | 0 | 0 | 112 | 6170 | 29.9 (23.5) | 64.1 | 177.9 (163.0) | 121.7 | 122.2 | 122.8 | 122.0 | 107.1 | 50.0 / 52.4 | 37.9 |

| Variant | Phase | Offered/s | Achieved/s | Failed | p50 ms | p99 ms | p99.9 ms | CPU ms/ok | CPU cores | Throttled ms | Steal % | Peak MiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| base-gc | steady-mixed | 30 | 29.9 | 0 | 28.8 | 5865.5 | 8085.5 | 25.76 | 0.75 | 15698 | 0.0 | 244.3 |
| re2b-combo-gc | steady-mixed | 30 | 30.0 | 0 | 5.5 | 208.5 | 467.7 | 15.25 | 0.45 | 3042 | 0.0 | 127.5 |
| re2b-heap-gc | steady-mixed | 30 | 30.0 | 0 | 5.5 | 338.2 | 711.7 | 14.72 | 0.44 | 3416 | 0.0 | 140.0 |
| base-gc | burst-mixed | closed | – | 1 | – | – | – | – | – | – | 0.0 | 255.9 |
| re2b-combo-gc | burst-mixed | closed | 93.2 | 0 | 84.3 | 1274.9 | 1608.7 | 10.87 | 0.99 | 15763 | 0.0 | 166.2 |
| re2b-heap-gc | burst-mixed | closed | 80.8 | 0 | 91.6 | 1620.0 | 2789.4 | 12.56 | 0.99 | 15685 | 0.0 | 177.9 |

## 512m memory, 1 vCPU

| Variant | Runs | OOM / failed runs | Failed requests | Contended runs | Heap limit MiB | Ready ms | Idle MiB (anon) | Idle PSS MiB | Peak MiB (anon) | After 10 s | After 30 s | After 60 s | After 120 s | Anon after 120 s | Heap used/total after 120 s | malloc free after 120 s |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| base-gc | 1 | 0 | 0 | 0 | – | 3215 | 34.2 (30.6) | 67.9 | 291.3 (281.8) | 279.7 | 161.6 | 162.3 | 162.3 | 154.1 | 49.9 / 54.2 | 82.0 |
| re2b-combo-gc | 1 | 0 | 0 | 0 | 268 | 4031 | 24.6 (21.4) | 58.5 | 192.5 (181.9) | 125.7 | 84.2 | 85.0 | 85.9 | 78.0 | 49.8 / 51.7 | 45.5 |
| re2b-heap-gc | 1 | 0 | 0 | 0 | 268 | 1963 | 34.1 (30.7) | 68.1 | 175.7 (166.9) | 115.8 | 116.2 | 117.2 | 116.2 | 106.2 | 50.3 / 54.9 | 34.7 |

| Variant | Phase | Offered/s | Achieved/s | Failed | p50 ms | p99 ms | p99.9 ms | CPU ms/ok | CPU cores | Throttled ms | Steal % | Peak MiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| base-gc | steady-mixed | 30 | 30.1 | 0 | 5.0 | 535.6 | 1245.2 | 13.73 | 0.41 | 3686 | 0.0 | 247.0 |
| re2b-combo-gc | steady-mixed | 30 | 30.0 | 0 | 5.4 | 102.6 | 219.9 | 12.84 | 0.38 | 1462 | 0.0 | 132.1 |
| re2b-heap-gc | steady-mixed | 30 | 30.0 | 0 | 4.7 | 67.3 | 156.9 | 11.10 | 0.33 | 925 | 0.0 | 128.1 |
| base-gc | burst-mixed | closed | 86.2 | 0 | 86.5 | 1561.6 | 2236.4 | 11.06 | 0.93 | 12951 | 0.0 | 291.3 |
| re2b-combo-gc | burst-mixed | closed | 128.4 | 0 | 68.2 | 867.8 | 994.3 | 7.84 | 0.99 | 15202 | 0.0 | 192.3 |
| re2b-heap-gc | burst-mixed | closed | 95.6 | 0 | 82.6 | 1383.4 | 2183.2 | 10.63 | 0.98 | 14343 | 0.0 | 175.7 |

