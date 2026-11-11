# static-i1 vs base-i1 (closed loop) — benchmarks/results/phase3a/ab-static

Median of valid rounds; change = B/A − 1 with 95% bootstrap interval; ✔ better, ✘ worse, · within noise.

| Workload | n | req/s A → B | CPU ms/req A → B | instr/req A → B | p95 ms A → B | p99 ms A → B | peak RSS MiB A → B | anon RSS MiB A → B |
|---|---|---|---|---|---|---|---|---|
| app/static-file | 5/5 | 47141 → 65137 ✔<br>+38.2% [36.5, 39.8] | 0.076 → 0.054 ✔<br>-29.2% [-29.9, -28.5] | 0.06M → 0.05M ✔<br>-12.6% [-14.9, -12.2] | 1.16 → 0.84 ✔<br>-27.6% [-29.7, -26.7] | 1.74 → 1.37 ✔<br>-21.3% [-23.8, -18.1] | 66 → 68 ✘<br>+3.2% [2.9, 4.1] | 26.6 → 26.8 ·<br>+0.8% [-2.2, 3.4] |
| app/prerendered (loadgen-limited) | 5/5 | 116524 → 117452 ·<br>+0.8% [-0.0, 2.1] | 0.020 → 0.020 ·<br>-0.1% [-1.6, 1.2] | 0.03M → 0.03M ✘<br>+0.3% [0.1, 0.7] | 0.40 → 0.40 ·<br>-0.5% [-2.5, 0.3] | 0.60 → 0.61 ·<br>+1.2% [-1.2, 2.8] | 66 → 68 ✘<br>+3.4% [2.9, 4.1] | 24.5 → 25.0 ·<br>+2.0% [0.0, 4.1] |
