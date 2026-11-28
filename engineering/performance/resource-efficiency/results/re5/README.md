# RE-5 raw results (process level)

These are measurements from one Fedora 45 host (i5-9400F, 6 cores), run
inside the VS Code Flatpak sandbox. None of them is a container
measurement. The reports interpret them:
[RE-5-BASELINE-AND-PROFILING](../../RE-5-BASELINE-AND-PROFILING.md) and
the other `RE-5-*.md` files.

## Binaries

Release builds (`cargo +1.98.0 build --release -p nextrs-cli`):

| Label | Commit | sha256 | Used in |
|---|---|---|---|
| baseline | `8d83d03` | `54daca719a771f788979837c5ee0d498e34ed9624c8ece95209186c9049a2c79` | everything |
| baseline profiling (line tables) | `8d83d03` | – | `alloc-sites/` |
| candidate 1 | `1ddb050` | `2845852453c9d7390258049d63f187e534f2a80bf2c6d9d658690bd32bdae720` | `prerender-candidate/` |
| candidate 2 | `1ddb050` + heartbeat | `d0bc4b6ca7e05f21d38469566b18e35f64ebaa33e346f7c1b641d93424ab0bd8` | `idle-cpu/` |
| candidate (final) | `8017043` | `0b2340ec37dd7e26726b43f0f2dff06fabafc57f1b702f899b65d014856ebe2e` | `alloc-candidate/`, `alloc-repeat/`, `cpu-ab/`, `gc-268/`, `gc-536/`, `prerender-quiet/` |
| rejected experiment | `8017043` + initial young size (reverted) | `f4779b02823e8778a88d6fd2bbb8943c22bd7000d97032103ff35a37e309b7df` | `gc-268-initial/` |

## Directories and commands

Run from the repository root:

```sh
export PATH=<node-24.18.0>/bin:$PATH
B=target/re5-baseline-rel/release/nextrs
C=target/re5-cand3-rel/release/nextrs
R=engineering/performance/resource-efficiency/results/re5
cc -O2 -shared -fPIC -o target/re5-alloc-count.so benchmarks/phase3a/alloc-count.c
cc -O2 -shared -fPIC -o target/re5-alloc-trace.so benchmarks/phase3a/alloc-trace.c
```

**Allocation profile** (`alloc-baseline/`, `alloc-candidate/`, `alloc-repeat/`):

```sh
node benchmarks/phase3a/alloc.mjs --nextrs $B --label baseline --preload target/re5-alloc-count.so --requests 3000 --out $R/alloc-baseline
```

**Allocation sites** (`alloc-sites/`; folded stacks gzipped):

```sh
node benchmarks/phase3a/alloc-sites.mjs --group app --workload ssr --nextrs <profiling build> --preload target/re5-alloc-trace.so --requests 2000 --out $R/alloc-sites
```

**CPU per request, A/B** (`cpu-ab/`):

```sh
node benchmarks/phase3a/run.mjs --out $R/cpu-ab --nextrs $B --label baseline --nextrs-b $C --label-b candidate --no-next --isolates 1 --groups app,pages --only static-file,static-gzip,prerendered,prerendered-gzip,ssr,ssr-gzip,rsc,action,action-multipart,isr-hit,pages-ssr,pages-api-get --rounds 3 --duration 10 --warmup 5 --connections 32 --open 0
```

**Young generation** (`gc-268/`, `gc-536/`):

```sh
node benchmarks/phase3a/gc.mjs --nextrs $C --young-api --heap-limit 268 --semi default,4,8,16 --isolates 1 --only ssr,rsc,action,streaming --rounds 2 --duration 10 --warmup 5 --connections 32 --out $R/gc-268
```

**Idle CPU** (`idle-cpu/`):

```sh
node benchmarks/resource/idle-cpu-process.mjs --bin $B --label baseline --settle 10 --window 60 --out $R/idle-cpu
```

**Prerender scale** (`prerender-*`). First build fixtures/next-prerender-scale:

```sh
NEXTRS_PRERENDER_ROUTES=N NEXTRS_PRERENDER_DIST=.next-N npx next build
```

Then:

```sh
node benchmarks/resource/prerender-scale.mjs --bin $C --sizes 10000,50000 --modes cache0,cache0r,cache32,eager --out $R/prerender-quiet
```

`prerender-baseline/` and `prerender-candidate/` predate the review's
driver fixes (gzip negotiated, warm page cache). Use their memory
columns only; `prerender-quiet/` is the corrected run.
