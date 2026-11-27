| Run | Type | Count | Total pause ms | Incremental steps ms | Mean pause ms | p99 ms | Max ms | Main-thread GC share of log span |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| base-gc-256m-1cpu-r1 | Scavenge | 75 | 3839 | 0 | 51.19 | 878.7 | 878.7 |  |
| base-gc-256m-1cpu-r1 | Mark-Compact | 11 | 636 | 2186 | 57.79 | 144.5 | 144.5 |  |
| base-gc-256m-1cpu-r1 | all | 86 | 4475 | 2186 | 52.04 | 878.7 | 878.7 | 4.32 % |
| base-gc-512m-1cpu-r1 | Scavenge | 121 | 1988 | 0 | 16.43 | 82.9 | 261.5 |  |
| base-gc-512m-1cpu-r1 | Mark-Compact | 12 | 532 | 708 | 44.31 | 178.5 | 178.5 |  |
| base-gc-512m-1cpu-r1 | all | 133 | 2520 | 708 | 18.95 | 178.5 | 261.5 | 2.33 % |
| re2b-combo-gc-256m-1cpu-r1 | Scavenge | 1703 | 3899 | 0 | 2.29 | 31.1 | 127.0 |  |
| re2b-combo-gc-256m-1cpu-r1 | Mark-Compact | 37 | 1063 | 1217 | 28.72 | 82.2 | 82.2 |  |
| re2b-combo-gc-256m-1cpu-r1 | all | 1740 | 4962 | 1217 | 2.85 | 40.3 | 127.0 | 4.48 % |
| re2b-combo-gc-512m-1cpu-r1 | Scavenge | 1018 | 3202 | 0 | 3.15 | 35.2 | 51.0 |  |
| re2b-combo-gc-512m-1cpu-r1 | Mark-Compact | 34 | 855 | 1100 | 25.14 | 80.4 | 80.4 |  |
| re2b-combo-gc-512m-1cpu-r1 | all | 1052 | 4057 | 1100 | 3.86 | 39.4 | 80.4 | 3.57 % |
| re2b-heap-gc-256m-1cpu-r1 | Scavenge | 1561 | 3795 | 0 | 2.43 | 30.2 | 142.2 |  |
| re2b-heap-gc-256m-1cpu-r1 | Mark-Compact | 33 | 1340 | 1617 | 40.59 | 282.6 | 282.6 |  |
| re2b-heap-gc-256m-1cpu-r1 | all | 1594 | 5135 | 1617 | 3.22 | 41.1 | 282.6 | 4.59 % |
| re2b-heap-gc-512m-1cpu-r1 | Scavenge | 883 | 2950 | 0 | 3.34 | 35.3 | 191.0 |  |
| re2b-heap-gc-512m-1cpu-r1 | Mark-Compact | 28 | 594 | 1066 | 21.22 | 83.3 | 83.3 |  |
| re2b-heap-gc-512m-1cpu-r1 | all | 911 | 3544 | 1066 | 3.89 | 39.1 | 191.0 | 3.37 % |
