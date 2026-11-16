// Sampled allocation backtraces for benchmarks/phase3a/alloc-sites.mjs
// (Phase 3A call-site attribution):
//
//   LD_PRELOAD=alloc-trace.so NEXTRS_ALLOC_TRACE=/path/trace.bin \
//     NEXTRS_ALLOC_SAMPLE=16 nextrs ...
//
// Every Nth allocation call (malloc, calloc, realloc, the aligned forms;
// one global counter) records its requested size and up to 32 return
// addresses (glibc `backtrace`, i.e. the unwinder through .eh_frame; it
// stops at V8 JIT frames, which have no unwind tables) into a per-thread
// buffer that is written to the trace file with write(2) when it fills.
// Recording is off until the file `<trace>.on` exists, so the driver can
// skip start-up and warm-up. At exit (or when `<trace>.flush` appears and
// another sample is taken) buffers are flushed; /proc/self/maps is copied
// to `<trace>.maps` once recording starts, for symbolisation.
//
// Counting and unwinding make allocation very slow: never use for timing.
//
//   cc -O2 -shared -fPIC -o alloc-trace.so alloc-trace.c
#define _GNU_SOURCE
#include <execinfo.h>
#include <fcntl.h>
#include <pthread.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>
#include <errno.h>
#include <stdio.h>

extern void *__libc_malloc(size_t);
extern void *__libc_calloc(size_t, size_t);
extern void *__libc_realloc(void *, size_t);
extern void __libc_free(void *);
extern void *__libc_memalign(size_t, size_t);

#define DEPTH 32
#define BUF_WORDS 8192

static int trace_fd = -1;
static uint64_t sample_every = 16;
static uint64_t calls;
static char on_path[4096];
static char maps_path[4096];
static volatile int recording;
static uint64_t checks;
static pthread_mutex_t write_lock = PTHREAD_MUTEX_INITIALIZER;

static __thread int busy;
static __thread uint64_t buf[BUF_WORDS];
static __thread size_t used;

static void flush_buf(void) {
    if (!used || trace_fd < 0) return;
    pthread_mutex_lock(&write_lock);
    const char *p = (const char *)buf;
    size_t left = used * sizeof(uint64_t);
    while (left) {
        ssize_t w = write(trace_fd, p, left);
        if (w <= 0) {
            if (errno == EINTR) continue;
            break;
        }
        p += w;
        left -= (size_t)w;
    }
    pthread_mutex_unlock(&write_lock);
    used = 0;
}

static void copy_maps(void) {
    int in = open("/proc/self/maps", O_RDONLY);
    int out = open(maps_path, O_WRONLY | O_CREAT | O_TRUNC, 0600);
    char tmp[8192];
    ssize_t n;
    while (in >= 0 && out >= 0 && (n = read(in, tmp, sizeof tmp)) > 0) {
        if (write(out, tmp, (size_t)n) != n) break;
    }
    if (in >= 0) close(in);
    if (out >= 0) close(out);
}

__attribute__((constructor)) static void setup(void) {
    const char *path = getenv("NEXTRS_ALLOC_TRACE");
    if (!path) return;
    const char *every = getenv("NEXTRS_ALLOC_SAMPLE");
    if (every && atoll(every) > 0) sample_every = (uint64_t)atoll(every);
    snprintf(on_path, sizeof on_path, "%s.on", path);
    snprintf(maps_path, sizeof maps_path, "%s.maps", path);
    trace_fd = open(path, O_WRONLY | O_CREAT | O_TRUNC | O_APPEND, 0600);
    busy = 1;
    void *warm[2];
    backtrace(warm, 2); // loads libgcc_s now, not inside a sampled call
    busy = 0;
}

__attribute__((destructor)) static void teardown(void) { flush_buf(); }

static void sample(size_t size) {
    if (trace_fd < 0 || busy) return;
    uint64_t c = __atomic_add_fetch(&calls, 1, __ATOMIC_RELAXED);
    if (!recording) {
        // Poll for the switch file every 65536 calls.
        if ((c & 0xffff) == 0 && __atomic_add_fetch(&checks, 1, __ATOMIC_RELAXED)) {
            struct stat st;
            if (stat(on_path, &st) == 0 && !__atomic_exchange_n(&recording, 1, __ATOMIC_SEQ_CST)) copy_maps();
        }
        return;
    }
    if (c % sample_every) return;
    busy = 1;
    void *frames[DEPTH + 2];
    int n = backtrace(frames, DEPTH + 2);
    // Drop sample() and the interposed function itself.
    int skip = n > 2 ? 2 : n;
    int k = n - skip;
    if (used + 2 + (size_t)k > BUF_WORDS) flush_buf();
    buf[used++] = ((uint64_t)k << 48) | (uint64_t)(size & 0xffffffffffffULL);
    buf[used++] = (uint64_t)gettid();
    for (int i = 0; i < k; i++) buf[used++] = (uint64_t)(uintptr_t)frames[skip + i];
    // Flush often enough that a killed server leaves usable data.
    if (used > BUF_WORDS / 2) flush_buf();
    busy = 0;
}

void *malloc(size_t n) { sample(n); return __libc_malloc(n); }
void *calloc(size_t a, size_t b) { sample(a * b); return __libc_calloc(a, b); }
void *realloc(void *p, size_t n) { sample(n); return __libc_realloc(p, n); }
void free(void *p) { __libc_free(p); }
void *memalign(size_t a, size_t n) { sample(n); return __libc_memalign(a, n); }
void *aligned_alloc(size_t a, size_t n) { sample(n); return __libc_memalign(a, n); }
int posix_memalign(void **out, size_t a, size_t n) {
    sample(n);
    void *p = __libc_memalign(a, n);
    if (!p) return ENOMEM;
    *out = p;
    return 0;
}
