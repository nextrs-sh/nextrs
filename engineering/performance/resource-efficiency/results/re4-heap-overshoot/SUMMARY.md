# re4-heap-overshoot

RE-4E heap-limit sweep with the near-heap-limit instrumentation. One
child process per case, Fedora 45 host (bare metal, no container),
i5-9400F, Rust 1.98.0 debug test binary, prebuilt release V8 150.4.0.
Branch `feat/re-4-resource-admission`, the code committed as `5011a7f`
(the sweep ran on that working tree just before the commit).

- `cases.jsonl`: one JSON object per recovered or served case (the
  child's `CASE` line): status, restarts, near-heap-limit calls, the
  episode (calls, first and last old-generation limit passed to the
  callback, unwind and replacement time), the next request's status, and
  the process's peak resident memory (`VmHWM`).
- `fatal.txt`: cases whose process aborted (exit 133, V8's fatal
  message). These print no `CASE` line.

Command (from the repository root):

```sh
B=$(ls -t target/debug/deps/heap_limits-* | grep -v '\.d$' | head -1)
for mode in heap-single heap-string; do for mib in 8 24 48 60 72 96 128 160 176 192 256; do
  NEXTRS_TEST_CHILD=heap-fault FAULT_MODE=$mode FAULT_MIB=$mib FAULT_HEAP_MIB=64 \
    $B child_heap_fault --exact --nocapture --test-threads=1
done; done
# and FAULT_HEAP_MIB=128 with heap-string 96 128 192 256
```

(`cargo +1.98.0 test -p nextrs-runtime --test heap_limits --no-run`
builds the binary.)
