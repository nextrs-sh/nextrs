
### node: container 27.1 millicores, main process 0.8 (window 300 s, ticks of 10 ms)

| Thread | Count | Millicores |
|---|---:|---:|
| `next-server (v1` | 1 | 0.67 |
| `V8Worker` | 4 | 0.13 |

### base: container 14.9 millicores, main process 9.3 (window 300 s, ticks of 10 ms)

| Thread | Count | Millicores |
|---|---:|---:|
| `nextrs-v8-0` | 1 | 6.93 |
| `nextrs-supervis` | 1 | 2.03 |
| `V8 DefaultWorke` | 1 | 0.20 |
| `tokio-rt-worker` | 1 | 0.07 |

### re2c-combo: container 14.8 millicores, main process 9.3 (window 300 s, ticks of 10 ms)

| Thread | Count | Millicores |
|---|---:|---:|
| `nextrs-v8-0` | 1 | 6.59 |
| `nextrs-supervis` | 1 | 2.03 |
| `tokio-rt-worker` | 1 | 0.50 |
| `V8 DefaultWorke` | 1 | 0.13 |

### re2c-balanced: container 14.8 millicores, main process 9.2 (window 300 s, ticks of 10 ms)

| Thread | Count | Millicores |
|---|---:|---:|
| `nextrs-v8-0` | 1 | 6.53 |
| `nextrs-supervis` | 1 | 2.00 |
| `tokio-rt-worker` | 1 | 0.50 |
| `V8 DefaultWorke` | 1 | 0.17 |
