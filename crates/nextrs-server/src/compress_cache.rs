//! Gzip copies of responses whose bytes cannot change, so that a gzip
//! client does not cost a fresh compression on every request.
//!
//! Only two kinds of body are ever cached (the server marks them with
//! [`Cacheable`]; everything else is compressed per request, streaming, as
//! before):
//!
//! - **prerendered App Router output held in memory since startup** (HTML,
//!   RSC payloads and segment prefetches of pages without ISR, without a
//!   Partial Prerendering shell, and not revalidated since: those are
//!   rendered by Next's handler instead and never reach this cache). The
//!   key is the identity of the in-memory copy, which is never replaced;
//! - **files under `/_next/static`**. The key is the file's canonical path
//!   together with what one `fstat` of the open handle says about it
//!   (device, inode, size, modification and change times, the latter
//!   changing on any write). Every request stats the file anyway, so a file
//!   replaced or rewritten on disk gets a new key and is compressed again;
//!   a fill whose file changed while it was read is not stored.
//!
//! Public files, dynamic output (SSR, route handlers, Server Actions, ISR,
//! PPR, streamed RSC), image optimizer results and anything the proxy
//! answered are never marked, so never cached.
//!
//! The cache holds gzip bytes only and is consulted only after the usual
//! negotiation chose gzip for a complete (200/404/…, never 206) response:
//! an identity client, a HEAD request, a range request or a response the
//! middleware rules leave uncompressed never reads it. Headers (Vary,
//! ETag, Cache-Control) are decided per request exactly as without the
//! cache; only the body is reused.
//!
//! Bounds: at most `capacity` bytes (compressed bytes plus a fixed overhead
//! and the key per entry), bodies above `max_entry` identity bytes are not
//! cached (compressed per request). When a fill takes the total above the
//! capacity, the least recently used entries are dropped until it is below
//! 7/8 of it (batched, so a cache under churn does not scan per request).
//! The key space is the build's own output, so a client cannot add keys.
//!
//! Concurrent misses for one key compress once: the others wait for that
//! result. A body above 64 KiB is compressed on the blocking pool.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use axum::body::Bytes;
use flate2::Compression;
use flate2::write::GzEncoder;

/// Default capacity in bytes.
pub const DEFAULT_CAPACITY: usize = 32 * 1024 * 1024;
/// Default largest identity body cached, in bytes.
pub const DEFAULT_MAX_ENTRY: usize = 4 * 1024 * 1024;
/// Counted per entry besides the compressed bytes (map slot, `Arc`, key).
const ENTRY_OVERHEAD: usize = 160;
/// Bodies above this are compressed on the blocking pool.
const INLINE_LIMIT: usize = 64 * 1024;

/// What a cached body is a copy of.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) enum Key {
    /// A prerendered response loaded at startup (`Cached::id`).
    Stored(u64),
    /// A `/_next/static` file as of one `fstat`.
    File(FileKey),
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(crate) struct FileKey {
    path: PathBuf,
    len: u64,
    modified_ns: i128,
    /// Unix: change time, device and inode (0 elsewhere).
    changed_ns: i128,
    dev: u64,
    ino: u64,
}

impl FileKey {
    /// The key for `path` as `metadata` (from the open handle) describes
    /// it; `None` without a modification time.
    pub(crate) fn of(path: &Path, metadata: &std::fs::Metadata) -> Option<Self> {
        let modified_ns = metadata
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos() as i128;
        #[cfg(unix)]
        let (changed_ns, dev, ino) = {
            use std::os::unix::fs::MetadataExt;
            (
                i128::from(metadata.ctime()) * 1_000_000_000 + i128::from(metadata.ctime_nsec()),
                metadata.dev(),
                metadata.ino(),
            )
        };
        #[cfg(not(unix))]
        let (changed_ns, dev, ino) = (0, 0, 0);
        Some(Self {
            path: path.to_path_buf(),
            len: metadata.len(),
            modified_ns,
            changed_ns,
            dev,
            ino,
        })
    }

    fn size(&self) -> usize {
        self.path.as_os_str().len()
    }
}

impl Key {
    fn size(&self) -> usize {
        match self {
            Self::Stored(_) => 0,
            Self::File(file) => file.size(),
        }
    }
}

/// A response body the cache may hold a gzip copy of: set by the server
/// on responses whose body is exactly `identity`, read by the compression
/// step.
#[derive(Clone)]
pub(crate) struct Cacheable {
    pub key: Key,
    pub identity: Bytes,
}

struct Slot {
    gzip: tokio::sync::OnceCell<Bytes>,
    last_used: AtomicU64,
}

#[derive(Default)]
struct Inner {
    map: HashMap<Key, Arc<Slot>>,
    /// Bytes accounted to the filled entries in `map`.
    held: usize,
}

/// See the module documentation.
pub struct CompressionCache {
    capacity: usize,
    max_entry: usize,
    inner: RwLock<Inner>,
    clock: AtomicU64,
    hits: AtomicU64,
    misses: AtomicU64,
    coalesced: AtomicU64,
    bypassed: AtomicU64,
    evictions: AtomicU64,
}

impl std::fmt::Debug for CompressionCache {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompressionCache")
            .field("capacity", &self.capacity)
            .field("max_entry", &self.max_entry)
            .finish_non_exhaustive()
    }
}

impl Default for CompressionCache {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY, DEFAULT_MAX_ENTRY)
    }
}

fn entry_cost(key: &Key, gzip: &Bytes) -> usize {
    gzip.len() + ENTRY_OVERHEAD + key.size()
}

impl CompressionCache {
    /// At most `capacity` bytes; identity bodies above `max_entry` bytes
    /// are not cached. A capacity of 0 turns the cache off.
    pub fn new(capacity: usize, max_entry: usize) -> Self {
        Self {
            capacity,
            max_entry,
            inner: RwLock::new(Inner::default()),
            clock: AtomicU64::new(0),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            coalesced: AtomicU64::new(0),
            bypassed: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
        }
    }

    /// `capacity` bytes, with the default largest entry.
    pub fn with_capacity(capacity: usize) -> Self {
        Self::new(capacity, DEFAULT_MAX_ENTRY)
    }

    pub fn disabled() -> Self {
        Self::new(0, 0)
    }

    pub(crate) fn enabled(&self) -> bool {
        self.capacity > 0 && self.max_entry > 0
    }

    /// Counters and sizes for the metrics endpoint.
    pub fn snapshot(&self) -> Vec<(&'static str, u64)> {
        let (entries, held) = {
            let inner = self.inner.read().unwrap_or_else(|e| e.into_inner());
            (inner.map.len() as u64, inner.held as u64)
        };
        vec![
            ("capacity_bytes", self.capacity as u64),
            ("max_entry_bytes", self.max_entry as u64),
            ("held_bytes", held),
            ("entries", entries),
            ("hits", self.hits.load(Ordering::Relaxed)),
            ("misses", self.misses.load(Ordering::Relaxed)),
            ("coalesced", self.coalesced.load(Ordering::Relaxed)),
            ("bypassed", self.bypassed.load(Ordering::Relaxed)),
            ("evictions", self.evictions.load(Ordering::Relaxed)),
        ]
    }

    /// The gzip copy of `identity` (stored under `key`), compressed now if
    /// it is not held yet. `None`: not cacheable here (too large, cache
    /// off, or compression failed); the caller compresses per request.
    pub(crate) async fn gzip(&self, key: Key, identity: Bytes) -> Option<Bytes> {
        if !self.enabled() || identity.len() > self.max_entry {
            self.bypassed.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        let tick = self.clock.fetch_add(1, Ordering::Relaxed);
        let existing = self
            .inner
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .map
            .get(&key)
            .cloned();
        let slot = match existing {
            Some(slot) => slot,
            None => self
                .inner
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .map
                .entry(key.clone())
                .or_insert_with(|| {
                    Arc::new(Slot {
                        gzip: tokio::sync::OnceCell::new(),
                        last_used: AtomicU64::new(tick),
                    })
                })
                .clone(),
        };
        slot.last_used.store(tick, Ordering::Relaxed);
        if let Some(gzip) = slot.gzip.get() {
            self.hits.fetch_add(1, Ordering::Relaxed);
            return Some(gzip.clone());
        }
        let filled = AtomicBool::new(false);
        let result = slot
            .gzip
            .get_or_try_init(|| {
                filled.store(true, Ordering::Relaxed);
                compress(identity)
            })
            .await;
        let Ok(gzip) = result else {
            // Nothing stored: waiters retry, later requests miss again.
            return None;
        };
        let gzip = gzip.clone();
        if filled.load(Ordering::Relaxed) {
            self.misses.fetch_add(1, Ordering::Relaxed);
            self.account(&key, &slot, &gzip);
        } else {
            self.coalesced.fetch_add(1, Ordering::Relaxed);
        }
        Some(gzip)
    }

    /// Counts a fill against the capacity and evicts if it is exceeded.
    fn account(&self, key: &Key, slot: &Arc<Slot>, gzip: &Bytes) {
        let cost = entry_cost(key, gzip);
        let mut inner = self.inner.write().unwrap_or_else(|e| e.into_inner());
        if !inner.map.get(key).is_some_and(|s| Arc::ptr_eq(s, slot)) {
            return;
        }
        if cost > self.capacity {
            // Served once, never held.
            inner.map.remove(key);
            self.bypassed.fetch_add(1, Ordering::Relaxed);
            return;
        }
        inner.held += cost;
        if inner.held <= self.capacity {
            return;
        }
        // Least recently used first, down to 7/8 of the capacity. Entries
        // still being filled cost nothing yet and stay.
        let target = self.capacity / 8 * 7;
        let mut filled: Vec<(u64, Key, usize)> = inner
            .map
            .iter()
            .filter_map(|(k, s)| {
                s.gzip.get().map(|g| {
                    (
                        s.last_used.load(Ordering::Relaxed),
                        k.clone(),
                        entry_cost(k, g),
                    )
                })
            })
            .collect();
        filled.sort_unstable_by_key(|(used, _, _)| *used);
        for (_, victim, victim_cost) in filled {
            if inner.held <= target {
                break;
            }
            inner.map.remove(&victim);
            inner.held -= victim_cost;
            self.evictions.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// One complete gzip member of `identity`, at the middleware's level.
async fn compress(identity: Bytes) -> Result<Bytes, std::io::Error> {
    fn run(identity: &[u8]) -> Result<Bytes, std::io::Error> {
        let mut encoder = GzEncoder::new(
            Vec::with_capacity(identity.len() / 3 + 64),
            Compression::default(),
        );
        encoder.write_all(identity)?;
        let mut out = encoder.finish()?;
        out.shrink_to_fit();
        Ok(Bytes::from(out))
    }
    if identity.len() <= INLINE_LIMIT {
        return run(&identity);
    }
    tokio::task::spawn_blocking(move || run(&identity))
        .await
        .map_err(std::io::Error::other)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn gunzip(bytes: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        flate2::read::GzDecoder::new(bytes)
            .read_to_end(&mut out)
            .unwrap();
        out
    }

    fn body(seed: u8, len: usize) -> Bytes {
        Bytes::from(
            (0..len)
                .map(|i| b"abcdefgh"[(i + seed as usize) % 8] ^ seed)
                .collect::<Vec<_>>(),
        )
    }

    fn stat(cache: &CompressionCache, name: &str) -> u64 {
        cache
            .snapshot()
            .into_iter()
            .find(|(n, _)| *n == name)
            .unwrap()
            .1
    }

    #[tokio::test]
    async fn compresses_once_then_hits() {
        let cache = CompressionCache::new(1 << 20, 1 << 20);
        let identity = body(1, 10_000);
        let first = cache.gzip(Key::Stored(1), identity.clone()).await.unwrap();
        assert_eq!(gunzip(&first), identity);
        let second = cache.gzip(Key::Stored(1), identity.clone()).await.unwrap();
        assert_eq!(first.as_ptr(), second.as_ptr(), "the same copy");
        assert_eq!(stat(&cache, "misses"), 1);
        assert_eq!(stat(&cache, "hits"), 1);
        assert_eq!(stat(&cache, "entries"), 1);
        // Another key is another entry, even for the same bytes.
        cache.gzip(Key::Stored(2), identity).await.unwrap();
        assert_eq!(stat(&cache, "misses"), 2);
    }

    #[tokio::test]
    async fn the_capacity_is_enforced() {
        // Poorly compressible 8 KiB bodies: about 8 KiB each compressed.
        let noise = |seed: u32| {
            let mut state = seed.max(1);
            Bytes::from(
                (0..8192)
                    .map(|_| {
                        state ^= state << 13;
                        state ^= state >> 17;
                        state ^= state << 5;
                        (state & 0xff) as u8
                    })
                    .collect::<Vec<_>>(),
            )
        };
        let capacity = 64 * 1024;
        let cache = CompressionCache::new(capacity, 1 << 20);
        for i in 0..100u32 {
            cache
                .gzip(Key::Stored(u64::from(i)), noise(i + 1))
                .await
                .unwrap();
            assert!(stat(&cache, "held_bytes") <= capacity as u64);
        }
        assert!(stat(&cache, "evictions") > 0);
        assert!(stat(&cache, "entries") < 100);
        // The most recent entry survived; the oldest did not.
        let held = |i: u64| {
            cache
                .inner
                .read()
                .unwrap()
                .map
                .get(&Key::Stored(i))
                .is_some_and(|s| s.gzip.get().is_some())
        };
        assert!(held(99));
        assert!(!held(0));
    }

    #[tokio::test]
    async fn recently_used_entries_survive_eviction() {
        let cache = CompressionCache::new(40 * 1024, 1 << 20);
        let mut state = 7u32;
        let mut noise = || {
            Bytes::from(
                (0..8192)
                    .map(|_| {
                        state ^= state << 13;
                        state ^= state >> 17;
                        state ^= state << 5;
                        (state & 0xff) as u8
                    })
                    .collect::<Vec<_>>(),
            )
        };
        let hot = noise();
        cache.gzip(Key::Stored(0), hot.clone()).await.unwrap();
        for i in 1..20 {
            cache.gzip(Key::Stored(i), noise()).await.unwrap();
            // Keep entry 0 in use.
            cache.gzip(Key::Stored(0), hot.clone()).await.unwrap();
        }
        assert_eq!(stat(&cache, "misses"), 20, "the hot entry never refilled");
    }

    #[tokio::test]
    async fn large_bodies_and_a_disabled_cache_bypass() {
        let cache = CompressionCache::new(1 << 20, 4096);
        assert!(cache.gzip(Key::Stored(1), body(2, 4097)).await.is_none());
        assert!(cache.gzip(Key::Stored(1), body(2, 4096)).await.is_some());
        let off = CompressionCache::disabled();
        assert!(!off.enabled());
        assert!(off.gzip(Key::Stored(1), body(2, 2000)).await.is_none());
        assert_eq!(stat(&off, "entries"), 0);
        // An entry larger than the whole capacity is served, not held.
        let tiny = CompressionCache::new(256, 1 << 20);
        let identity = body(3, 50_000);
        let gzip = tiny.gzip(Key::Stored(1), identity.clone()).await.unwrap();
        assert_eq!(gunzip(&gzip), identity);
        assert_eq!(stat(&tiny, "entries"), 0);
        assert_eq!(stat(&tiny, "held_bytes"), 0);
    }

    /// Simultaneous misses for one key compress once.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_misses_compress_once() {
        let cache = Arc::new(CompressionCache::new(8 << 20, 4 << 20));
        // Above the inline limit: filled on the blocking pool, so the
        // other tasks really wait.
        let identity = body(5, 1 << 20);
        let tasks: Vec<_> = (0..32)
            .map(|_| {
                let cache = cache.clone();
                let identity = identity.clone();
                tokio::spawn(async move { cache.gzip(Key::Stored(9), identity).await.unwrap() })
            })
            .collect();
        let mut outputs = Vec::new();
        for task in tasks {
            outputs.push(task.await.unwrap());
        }
        assert_eq!(stat(&cache, "misses"), 1);
        assert_eq!(
            stat(&cache, "hits") + stat(&cache, "coalesced"),
            31,
            "{:?}",
            cache.snapshot()
        );
        assert!(outputs.iter().all(|o| o == &outputs[0]));
        assert_eq!(gunzip(&outputs[0]), identity);
    }

    #[test]
    fn file_keys_change_with_the_file() {
        let dir = std::env::temp_dir().join(format!("nextrs-cc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.js");
        std::fs::write(&path, "one").unwrap();
        let key = |p: &Path| FileKey::of(p, &std::fs::metadata(p).unwrap()).unwrap();
        let first = key(&path);
        assert_eq!(first, key(&path), "stable while unchanged");
        // Same size, rewritten in place: the change time moves.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&path, "two").unwrap();
        assert_ne!(first, key(&path));
        // Same content at another path: another key.
        let other = dir.join("b.js");
        std::fs::write(&other, "two").unwrap();
        assert_ne!(key(&path), key(&other));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
