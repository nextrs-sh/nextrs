# engineering/performance/resource-efficiency/results/baseline-r1

## 128m memory, 0.5 vCPU

| Variant | Runs | OOM / failed runs | Failed requests | Ready ms | Idle MiB (anon) | Idle PSS MiB | Peak MiB (anon) | After 10 s | After 60 s | After 120 s (anon) | Heap used/total after 120 s | malloc free after 120 s |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| base | 1 | r1: OOM | 1 | 7263 | 33.9 (30.7) | 67.9 | 126.7 (120.0) | – | – | – (–) | – / – | – |
| node | 1 | r1: OOM | 3 | 10340 | 51.0 (46.8) | 67.2 | 128.0 (122.1) | – | – | – (–) | – / – | – |

| Variant | Phase | Offered/s | Achieved/s | Failed | p50 ms | p99 ms | p99.9 ms | CPU ms/ok | CPU cores | Peak MiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|

## 1g memory, 2 vCPU

| Variant | Runs | OOM / failed runs | Failed requests | Ready ms | Idle MiB (anon) | Idle PSS MiB | Peak MiB (anon) | After 10 s | After 60 s | After 120 s (anon) | Heap used/total after 120 s | malloc free after 120 s |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| base | 1 | 0 | 0 | 2520 | 34.6 (31.2) | 68.3 | 318.3 (304.9) | 317.9 | 185.2 | 185.2 (172.8) | 47.7 / 52.2 | 100.9 |
| node | 1 | 0 | 0 | 12401 | 63.2 (47.0) | 67.9 | 283.5 (254.9) | 256.4 | 256.8 | 195.5 (165.9) | – / – | – |

| Variant | Phase | Offered/s | Achieved/s | Failed | p50 ms | p99 ms | p99.9 ms | CPU ms/ok | CPU cores | Peak MiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| base | steady-static | 200 | 200.0 | 0 | 1.4 | 3.1 | 45.5 | 0.99 | 0.20 | 179.3 |
| node | steady-static | 200 | 200.1 | 0 | 9.1 | 578.0 | 694.3 | 6.17 | 1.22 | 252.1 |
| base | steady-rsc | 30 | 30.0 | 0 | 4.4 | 9.0 | 18.3 | 5.27 | 0.16 | 129.2 |
| node | steady-rsc | 30 | 30.0 | 0 | 5.9 | 14.7 | 57.1 | 7.62 | 0.23 | 254.2 |
| base | steady-ssr | 5 | 5.0 | 0 | 58.6 | 136.6 | 312.3 | 78.77 | 0.39 | 241.4 |
| node | steady-ssr | 5 | 5.0 | 0 | 80.7 | 252.4 | 396.8 | 111.89 | 0.55 | 256.6 |
| base | steady-api | 15 | 15.0 | 0 | 9.3 | 22.4 | 174.1 | 8.99 | 0.13 | 235.5 |
| node | steady-api | 15 | 15.0 | 0 | 12.4 | 31.4 | 55.8 | 14.41 | 0.21 | 267.3 |
| base | steady-mixed | 30 | 30.0 | 0 | 4.2 | 67.0 | 167.8 | 9.59 | 0.29 | 248.8 |
| node | steady-mixed | 30 | 30.0 | 0 | 11.6 | 855.6 | 1804.3 | 20.34 | 0.60 | 258.9 |
| base | burst-mixed | closed | 217.5 | 0 | 40.4 | 487.2 | 762.4 | 6.53 | 1.40 | 318.3 |
| node | burst-mixed | closed | 80.6 | 0 | 133.0 | 900.1 | 1404.9 | 17.39 | 1.36 | 271.2 |

## 256m memory, 0.5 vCPU

| Variant | Runs | OOM / failed runs | Failed requests | Ready ms | Idle MiB (anon) | Idle PSS MiB | Peak MiB (anon) | After 10 s | After 60 s | After 120 s (anon) | Heap used/total after 120 s | malloc free after 120 s |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| base | 1 | r1: OOM | 81 | 2170 | 34.3 (30.6) | 67.5 | 255.2 (247.9) | – | – | – (–) | – / – | – |
| node | 1 | 0 | 0 | 3986 | 51.3 (46.1) | 67.1 | 226.3 (217.0) | 198.9 | 199.1 | 169.2 (158.1) | – / – | – |

| Variant | Phase | Offered/s | Achieved/s | Failed | p50 ms | p99 ms | p99.9 ms | CPU ms/ok | CPU cores | Peak MiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| base | steady-static | 200 | 200.1 | 0 | 2.2 | 191.7 | 267.5 | 1.30 | 0.26 | 178.7 |
| node | steady-static | 200 | 97.3 | 0 | 12419.1 | 23248.9 | 23380.0 | 5.16 | 0.50 | 181.9 |
| base | steady-rsc | 30 | 30.0 | 0 | 4.4 | 40.3 | 78.0 | 5.38 | 0.16 | 125.1 |
| node | steady-rsc | 30 | 30.0 | 0 | 5.6 | 278.0 | 564.2 | 7.53 | 0.22 | 178.6 |
| base | steady-ssr | 5 | 3.2 | 81 | 119.7 | 833.5 | 927.7 | – | – | 255.2 |
| node | steady-ssr | 5 | 4.6 | 0 | 5644.3 | 7544.8 | 7761.9 | 111.27 | 0.49 | 215.3 |
| node | steady-api | 15 | 15.0 | 0 | 11.9 | 821.2 | 1139.7 | 13.86 | 0.21 | 214.2 |
| node | steady-mixed | 30 | 29.4 | 0 | 2496.5 | 6189.1 | 6770.7 | 17.11 | 0.50 | 220.3 |
| node | burst-mixed | closed | 19.2 | 0 | 513.5 | 3889.2 | 4689.9 | 26.36 | 0.49 | 225.3 |

## 256m memory, 1 vCPU

| Variant | Runs | OOM / failed runs | Failed requests | Ready ms | Idle MiB (anon) | Idle PSS MiB | Peak MiB (anon) | After 10 s | After 60 s | After 120 s (anon) | Heap used/total after 120 s | malloc free after 120 s |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| base | 1 | r1: OOM | 112 | 1675 | 34.4 (30.9) | 68.0 | 249.8 (240.9) | – | – | – (–) | – / – | – |
| node | 1 | 0 | 0 | 2092 | 52.1 (47.6) | 68.5 | 221.3 (210.4) | 194.0 | 194.0 | 165.8 (153.9) | – / – | – |

| Variant | Phase | Offered/s | Achieved/s | Failed | p50 ms | p99 ms | p99.9 ms | CPU ms/ok | CPU cores | Peak MiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| base | steady-static | 200 | 200.0 | 0 | 2.3 | 3.5 | 10.5 | 1.19 | 0.24 | 178.3 |
| node | steady-static | 200 | 200.1 | 0 | 16.5 | 533.5 | 676.9 | 4.71 | 0.93 | 187.9 |
| base | steady-rsc | 30 | 30.0 | 0 | 5.5 | 9.1 | 15.7 | 5.96 | 0.18 | 128.3 |
| node | steady-rsc | 30 | 30.0 | 0 | 6.9 | 23.4 | 66.8 | 8.57 | 0.26 | 184.1 |
| base | steady-ssr | 5 | 5.0 | 0 | 54.5 | 102.2 | 123.5 | 66.61 | 0.33 | 243.0 |
| node | steady-ssr | 5 | 5.0 | 0 | 66.8 | 502.3 | 714.8 | 92.71 | 0.46 | 200.9 |
| base | steady-api | 15 | 15.0 | 0 | 9.6 | 17.1 | 53.0 | 8.91 | 0.13 | 233.2 |
| node | steady-api | 15 | 15.0 | 0 | 12.0 | 36.0 | 130.9 | 13.11 | 0.20 | 180.7 |
| base | steady-mixed | 30 | 30.0 | 0 | 5.1 | 72.8 | 97.6 | 9.99 | 0.30 | 248.4 |
| node | steady-mixed | 30 | 30.0 | 0 | 9.0 | 153.7 | 268.3 | 16.37 | 0.49 | 206.6 |
| base | burst-mixed | closed | 16.9 | 112 | 59.7 | 681.0 | 890.9 | – | – | 249.8 |
| node | burst-mixed | closed | 70.4 | 0 | 150.4 | 981.5 | 1260.5 | 14.26 | 0.99 | 198.8 |

## 512m memory, 1 vCPU

| Variant | Runs | OOM / failed runs | Failed requests | Ready ms | Idle MiB (anon) | Idle PSS MiB | Peak MiB (anon) | After 10 s | After 60 s | After 120 s (anon) | Heap used/total after 120 s | malloc free after 120 s |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| base | 1 | 0 | 0 | 2183 | 33.3 (30.2) | 66.9 | 292.1 (281.5) | 284.4 | 162.4 | 163.7 (153.3) | 47.6 / 51.9 | 84.9 |
| node | 1 | 0 | 0 | 1912 | 52.8 (48.2) | 69.2 | 222.0 (213.7) | 196.4 | 196.1 | 168.8 (157.9) | – / – | – |

| Variant | Phase | Offered/s | Achieved/s | Failed | p50 ms | p99 ms | p99.9 ms | CPU ms/ok | CPU cores | Peak MiB |
|---|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| base | steady-static | 200 | 200.1 | 0 | 1.8 | 7.5 | 46.0 | 1.03 | 0.20 | 179.9 |
| node | steady-static | 200 | 200.0 | 0 | 7.6 | 350.7 | 468.0 | 4.59 | 0.91 | 189.7 |
| base | steady-rsc | 30 | 30.0 | 0 | 4.5 | 9.5 | 36.4 | 5.38 | 0.16 | 128.9 |
| node | steady-rsc | 30 | 30.0 | 0 | 6.8 | 20.0 | 60.4 | 8.02 | 0.24 | 191.6 |
| base | steady-ssr | 5 | 5.0 | 0 | 56.6 | 104.6 | 115.7 | 71.02 | 0.35 | 235.6 |
| node | steady-ssr | 5 | 5.0 | 0 | 126.0 | 6967.3 | 7237.6 | 145.99 | 0.72 | 194.2 |
| base | steady-api | 15 | 15.0 | 0 | 9.5 | 16.4 | 34.5 | 9.01 | 0.13 | 226.3 |
| node | steady-api | 15 | 15.0 | 0 | 12.2 | 84.7 | 159.7 | 14.89 | 0.22 | 216.7 |
| base | steady-mixed | 30 | 30.0 | 0 | 4.8 | 70.9 | 123.6 | 10.15 | 0.30 | 239.3 |
| node | steady-mixed | 30 | 30.0 | 0 | 35.2 | 4685.8 | 6156.3 | 23.74 | 0.70 | 218.1 |
| base | burst-mixed | closed | 144.3 | 0 | 59.3 | 747.5 | 934.4 | 7.02 | 0.99 | 292.1 |
| node | burst-mixed | closed | 52.6 | 0 | 190.0 | 1627.1 | 1882.1 | 19.27 | 0.98 | 222.0 |

