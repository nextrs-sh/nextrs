// Allocation counter for benchmarks/phase3a/alloc.mjs (Phase 3A):
// LD_PRELOAD=alloc-count.so NEXTRS_ALLOC_COUNTERS=/path/file nextrs ...
//
// Interposes the glibc allocation functions, forwards to glibc's own
// entry points (__libc_*, so no dlsym and no recursion), and counts calls
// and requested bytes in a file mapped shared (eight uint64 counters), so
// another process can read them at any time. Counts Rust and C/C++
// allocations; V8's JavaScript heap is managed separately (see the heap
// metrics). Relaxed atomics on a shared cache line make malloc slower:
// use this for counts, never for timing.
//
//   cc -O2 -shared -fPIC -o alloc-count.so alloc-count.c
#define _GNU_SOURCE
#include <errno.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdlib.h>
#include <sys/mman.h>
#include <unistd.h>

extern void *__libc_malloc(size_t);
extern void *__libc_calloc(size_t, size_t);
extern void *__libc_realloc(void *, size_t);
extern void __libc_free(void *);
extern void *__libc_memalign(size_t, size_t);

enum { MALLOCS, CALLOCS, REALLOCS, FREES, ALIGNED, BYTES, REALLOC_BYTES, N };
static uint64_t fallback[N];
static uint64_t *counters = fallback;

__attribute__((constructor)) static void setup(void) {
    const char *path = getenv("NEXTRS_ALLOC_COUNTERS");
    if (!path) return;
    int fd = open(path, O_RDWR | O_CREAT | O_TRUNC, 0600);
    if (fd < 0) return;
    if (ftruncate(fd, sizeof(uint64_t) * N) == 0) {
        void *p = mmap(NULL, sizeof(uint64_t) * N, PROT_READ | PROT_WRITE, MAP_SHARED, fd, 0);
        if (p != MAP_FAILED) counters = p;
    }
    close(fd);
}

#define ADD(i, v) __atomic_fetch_add(&counters[i], (uint64_t)(v), __ATOMIC_RELAXED)

void *malloc(size_t n) { ADD(MALLOCS, 1); ADD(BYTES, n); return __libc_malloc(n); }
void *calloc(size_t a, size_t b) { ADD(CALLOCS, 1); ADD(BYTES, a * b); return __libc_calloc(a, b); }
void *realloc(void *p, size_t n) { ADD(REALLOCS, 1); ADD(REALLOC_BYTES, n); return __libc_realloc(p, n); }
void free(void *p) { if (p) ADD(FREES, 1); __libc_free(p); }
void *memalign(size_t a, size_t n) { ADD(ALIGNED, 1); ADD(BYTES, n); return __libc_memalign(a, n); }
void *aligned_alloc(size_t a, size_t n) { ADD(ALIGNED, 1); ADD(BYTES, n); return __libc_memalign(a, n); }
int posix_memalign(void **out, size_t a, size_t n) {
    ADD(ALIGNED, 1);
    ADD(BYTES, n);
    void *p = __libc_memalign(a, n);
    if (!p) return ENOMEM;
    *out = p;
    return 0;
}
