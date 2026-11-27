# engineering/performance/resource-efficiency/results/re23-confirm-young

## 1g memory, 2 vCPU

| Variant | Runs | OOM / failed runs | Failed requests | Contended runs | Heap limit MiB | Ready ms | Idle MiB (anon) | Idle PSS MiB | Peak MiB (anon) | After 10 s | After 30 s | After 60 s | After 120 s | After 300 s | Anon after 300 s | Heap used/total after 300 s | malloc free after 300 s |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| re2c-combo | 1 | 0 | 0 | 0 | 536 | 1881 | 24.0 (21.4) | 60.7 | 222.8 (212.6) | 102.9 | 134.5 | 137.8 | 137.8 | 138.8 | 114.3 | 49.4 / 52.1 | 59.2 |
| re2c-combo-semi16 | 1 | 0 | 0 | 0 | 560 | 1829 | 24.3 (21.3) | 60.3 | 230.8 (219.7) | 99.3 | 130.2 | 134.3 | 133.8 | 134.8 | 113.2 | 49.3 / 52.1 | 56.1 |
| re2c-combo-semi8 | 1 | 0 | 0 | 0 | 536 | 1839 | 24.4 (21.2) | 60.4 | 223.4 (208.6) | 107.5 | 139.4 | 143.3 | 142.8 | 145.0 | 117.7 | 49.3 / 51.9 | 61.2 |

| Variant | Phase | Offered/s | Achieved/s | Failed | p50 ms | p99 ms | p99.9 ms | CPU ms/ok | CPU cores | Throttled ms | Steal % | Peak MiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| re2c-combo | steady-static | 200 | 200.1 | 0 | 2.2 | 3.3 | 5.6 | 1.26 | 0.25 | 0 | 0.0 | 129.5 |
| re2c-combo-semi16 | steady-static | 200 | 200.1 | 0 | 2.1 | 3.3 | 5.7 | 1.25 | 0.25 | 0 | 0.0 | 152.7 |
| re2c-combo-semi8 | steady-static | 200 | 200.0 | 0 | 2.2 | 3.4 | 6.0 | 1.26 | 0.25 | 0 | 0.0 | 146.9 |
| re2c-combo | steady-rsc | 30 | 30.0 | 0 | 5.6 | 9.3 | 12.6 | 6.09 | 0.18 | 0 | 0.0 | 113.3 |
| re2c-combo-semi16 | steady-rsc | 30 | 30.0 | 0 | 5.4 | 8.7 | 12.8 | 5.87 | 0.17 | 0 | 0.0 | 125.3 |
| re2c-combo-semi8 | steady-rsc | 30 | 30.0 | 0 | 5.5 | 9.4 | 12.2 | 5.89 | 0.18 | 0 | 0.0 | 119.9 |
| re2c-combo | steady-ssr | 5 | 5.0 | 0 | 55.9 | 76.0 | 80.7 | 69.96 | 0.35 | 0 | 0.0 | 147.3 |
| re2c-combo-semi16 | steady-ssr | 5 | 5.0 | 0 | 54.8 | 76.9 | 84.7 | 69.30 | 0.34 | 0 | 0.0 | 180.4 |
| re2c-combo-semi8 | steady-ssr | 5 | 5.0 | 0 | 55.8 | 75.0 | 92.4 | 69.48 | 0.34 | 0 | 0.0 | 149.5 |
| re2c-combo | steady-api | 15 | 15.0 | 0 | 9.5 | 14.2 | 21.6 | 9.24 | 0.14 | 0 | 0.0 | 147.9 |
| re2c-combo-semi16 | steady-api | 15 | 15.0 | 0 | 9.4 | 15.4 | 23.4 | 9.62 | 0.14 | 0 | 0.0 | 165.5 |
| re2c-combo-semi8 | steady-api | 15 | 15.0 | 0 | 9.3 | 15.0 | 50.3 | 9.25 | 0.14 | 0 | 0.0 | 149.8 |
| re2c-combo | steady-mixed | 30 | 30.0 | 0 | 4.9 | 60.8 | 68.7 | 10.21 | 0.30 | 0 | 0.0 | 147.6 |
| re2c-combo-semi16 | steady-mixed | 30 | 30.0 | 0 | 5.0 | 62.5 | 74.3 | 10.23 | 0.31 | 0 | 0.0 | 173.0 |
| re2c-combo-semi8 | steady-mixed | 30 | 30.0 | 0 | 4.9 | 58.9 | 66.4 | 10.25 | 0.31 | 0 | 0.0 | 150.9 |
| re2c-combo | burst-mixed | closed | 194.9 | 0 | 45.4 | 550.4 | 657.9 | 7.33 | 1.41 | 0 | 0.0 | 222.8 |
| re2c-combo-semi16 | burst-mixed | closed | 203.1 | 0 | 43.6 | 524.8 | 588.3 | 7.03 | 1.41 | 0 | 0.0 | 229.8 |
| re2c-combo-semi8 | burst-mixed | closed | 198.2 | 0 | 45.2 | 547.3 | 611.8 | 7.18 | 1.40 | 0 | 0.0 | 223.4 |
| re2c-combo | steady-action | 5 | 5.0 | 0 | 24.8 | 33.8 | 36.9 | 33.76 | 0.17 | 0 | 0.0 | 216.9 |
| re2c-combo-semi16 | steady-action | 5 | 5.0 | 0 | 25.1 | 33.9 | 73.1 | 34.11 | 0.17 | 0 | 0.0 | 230.8 |
| re2c-combo-semi8 | steady-action | 5 | 5.0 | 0 | 26.5 | 37.7 | 47.8 | 34.78 | 0.17 | 0 | 0.0 | 214.1 |

## 512m memory, 1 vCPU

| Variant | Runs | OOM / failed runs | Failed requests | Contended runs | Heap limit MiB | Ready ms | Idle MiB (anon) | Idle PSS MiB | Peak MiB (anon) | After 10 s | After 30 s | After 60 s | After 120 s | After 300 s | Anon after 300 s | Heap used/total after 300 s | malloc free after 300 s |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| re2c-combo | 1 | 0 | 0 | 0 | 268 | 1803 | 30.5 (20.3) | 59.5 | 196.4 (170.7) | 97.9 | 98.5 | 98.9 | 96.1 | 97.3 | 75.6 | 49.5 / 51.6 | 37.7 |
| re2c-combo-semi16 | 1 | 0 | 0 | 0 | 304 | 1968 | 24.8 (21.4) | 60.2 | 212.4 (202.5) | 126.3 | 118.6 | 119.2 | 119.5 | 120.4 | 100.5 | 49.1 / 51.6 | 45.7 |
| re2c-combo-semi8 | 1 | 0 | 0 | 0 | 280 | 1844 | 24.8 (20.5) | 59.3 | 191.6 (178.5) | 85.0 | 85.4 | 86.6 | 86.3 | 87.0 | 77.0 | 49.1 / 51.9 | 37.9 |

| Variant | Phase | Offered/s | Achieved/s | Failed | p50 ms | p99 ms | p99.9 ms | CPU ms/ok | CPU cores | Throttled ms | Steal % | Peak MiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| re2c-combo | steady-static | 200 | 200.1 | 0 | 2.2 | 4.3 | 82.8 | 1.24 | 0.25 | 136 | 0.0 | 134.8 |
| re2c-combo-semi16 | steady-static | 200 | 200.0 | 0 | 2.1 | 3.5 | 29.7 | 1.15 | 0.23 | 145 | 0.0 | 140.2 |
| re2c-combo-semi8 | steady-static | 200 | 200.1 | 0 | 2.3 | 3.5 | 23.4 | 1.19 | 0.24 | 150 | 0.0 | 131.7 |
| re2c-combo | steady-rsc | 30 | 30.0 | 0 | 5.3 | 10.2 | 32.2 | 5.84 | 0.17 | 195 | 0.0 | 126.1 |
| re2c-combo-semi16 | steady-rsc | 30 | 30.0 | 0 | 5.5 | 9.5 | 29.0 | 6.01 | 0.18 | 154 | 0.0 | 122.2 |
| re2c-combo-semi8 | steady-rsc | 30 | 30.0 | 0 | 5.6 | 9.0 | 12.5 | 5.99 | 0.18 | 91 | 0.0 | 113.5 |
| re2c-combo | steady-ssr | 5 | 5.0 | 0 | 56.0 | 135.2 | 328.2 | 71.85 | 0.36 | 796 | 0.0 | 147.1 |
| re2c-combo-semi16 | steady-ssr | 5 | 5.0 | 0 | 56.1 | 74.7 | 86.0 | 70.04 | 0.35 | 815 | 0.0 | 150.7 |
| re2c-combo-semi8 | steady-ssr | 5 | 5.0 | 0 | 55.3 | 103.7 | 110.9 | 70.14 | 0.35 | 947 | 0.0 | 139.0 |
| re2c-combo | steady-api | 15 | 15.0 | 0 | 9.3 | 13.8 | 17.2 | 9.36 | 0.14 | 0 | 0.0 | 151.2 |
| re2c-combo-semi16 | steady-api | 15 | 15.0 | 0 | 9.6 | 15.4 | 19.9 | 9.49 | 0.14 | 0 | 0.0 | 159.0 |
| re2c-combo-semi8 | steady-api | 15 | 15.0 | 0 | 9.6 | 16.2 | 106.0 | 9.50 | 0.14 | 48 | 0.0 | 134.8 |
| re2c-combo | steady-mixed | 30 | 30.0 | 0 | 4.7 | 63.5 | 134.4 | 10.50 | 0.31 | 805 | 0.0 | 148.2 |
| re2c-combo-semi16 | steady-mixed | 30 | 30.0 | 0 | 5.0 | 58.9 | 74.3 | 10.24 | 0.31 | 743 | 0.0 | 151.4 |
| re2c-combo-semi8 | steady-mixed | 30 | 30.0 | 0 | 5.2 | 61.9 | 68.7 | 10.44 | 0.31 | 729 | 0.0 | 136.7 |
| re2c-combo | burst-mixed | closed | 122.6 | 0 | 72.3 | 906.2 | 981.5 | 8.22 | 0.99 | 15375 | 0.0 | 196.4 |
| re2c-combo-semi16 | burst-mixed | closed | 134.9 | 0 | 65.3 | 858.6 | 926.7 | 7.48 | 0.99 | 14672 | 0.0 | 210.8 |
| re2c-combo-semi8 | burst-mixed | closed | 130.5 | 0 | 68.6 | 867.8 | 946.7 | 7.72 | 0.99 | 14348 | 0.0 | 191.6 |
| re2c-combo | steady-action | 5 | 5.0 | 0 | 25.4 | 44.1 | 90.4 | 35.55 | 0.18 | 380 | 0.0 | 180.3 |
| re2c-combo-semi16 | steady-action | 5 | 5.0 | 0 | 25.8 | 32.7 | 35.9 | 35.14 | 0.17 | 2 | 0.0 | 212.4 |
| re2c-combo-semi8 | steady-action | 5 | 5.0 | 0 | 26.4 | 36.4 | 55.4 | 36.28 | 0.18 | 725 | 0.0 | 189.0 |

