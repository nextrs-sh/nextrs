//! File-backed prerendered output (RE-5B, opt-in: `--prerender-cache`).
//!
//! By default the server reads every prerendered App Router page that it
//! serves itself (HTML, RSC payload, segment prefetches; no ISR, no PPR)
//! and every static Pages Router page into memory at startup, so its idle
//! memory grows with the build: about 84 KiB per route for a page the
//! size of Velasia's median one, 0.8 GiB at 10 000 routes.
//!
//! With a store, startup records only each file's length and modification
//! time. A body is read when a request needs it and kept in a bounded,
//! least-recently-used cache of `capacity` bytes, shared by all requests
//! (0: nothing is kept). Startup and idle memory then no longer grow with
//! the size of the prerendered output, only with the number of entries
//! (their headers and paths).
//!
//! * **Same responses.** The bytes served are the file's bytes, which the
//!   eager server would have read at startup: a file whose length or
//!   modification time changed since startup is refused (500, logged),
//!   never served. ETags are computed from the bytes with the same
//!   function, the first time an entry is read, and kept.
//! * **One read per entry at a time.** Concurrent requests for an entry
//!   that is not cached wait for one read (a lock per stripe of entries).
//! * **Bounded.** The cache never holds more than `capacity` bytes; an
//!   entry larger than the whole cache is read for its response and not
//!   kept. Evicted bytes stay alive only while a response still sends
//!   them. Reads that are not kept are reserved in the native buffer
//!   budget when there is one, as static files are.
//! * **Compression cache.** Entries keep the id they were given at
//!   startup, so gzip copies stay valid across reloads (the bytes are the
//!   same, as above).
//!
//! Paths come from the build's manifests at startup, never from requests.

use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use axum::body::Bytes;

use crate::native_budget::{BufferKind, NativeBufferBudget};

/// Locks that serialize reads of one entry (entries share a stripe by id).
const STRIPES: usize = 64;

/// A prerendered body on disk, as recorded at startup.
#[derive(Debug, Clone)]
pub(crate) struct FileSource {
    pub path: PathBuf,
    pub len: u64,
    pub modified: Option<SystemTime>,
}

#[derive(Debug)]
pub(crate) enum LoadError {
    /// The file could not be read.
    Io(std::io::Error),
    /// Its length or modification time is not what startup recorded.
    Changed,
    /// The native buffer budget had no room in time.
    Busy,
}

impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "{error}"),
            Self::Changed => f.write_str("the file changed after startup"),
            Self::Busy => f.write_str("the native buffer budget is full"),
        }
    }
}

#[derive(Default)]
struct Lru {
    entries: HashMap<u64, (Bytes, u64)>,
    /// Use tick → entry id, oldest first.
    order: BTreeMap<u64, u64>,
    tick: u64,
    used: u64,
}

impl Lru {
    fn get(&mut self, id: u64) -> Option<Bytes> {
        let (bytes, tick) = self.entries.get_mut(&id)?;
        self.order.remove(tick);
        self.tick += 1;
        *tick = self.tick;
        self.order.insert(self.tick, id);
        Some(bytes.clone())
    }

    /// Keeps `bytes` (which must fit in `capacity`), evicting the least
    /// recently used entries first; returns how many were evicted.
    fn insert(&mut self, id: u64, bytes: Bytes, capacity: u64) -> u64 {
        let len = bytes.len() as u64;
        let mut evicted = 0;
        while self.used + len > capacity {
            let Some((_, oldest)) = self.order.pop_first() else {
                break;
            };
            if let Some((old, _)) = self.entries.remove(&oldest) {
                self.used -= old.len() as u64;
                evicted += 1;
            }
        }
        self.tick += 1;
        self.order.insert(self.tick, id);
        self.used += len;
        if let Some((old, tick)) = self.entries.insert(id, (bytes, self.tick)) {
            // Not reached (callers check first under the stripe lock), but
            // keep the accounting exact if it is.
            self.order.remove(&tick);
            self.used -= old.len() as u64;
        }
        evicted
    }
}

pub struct PrerenderStore {
    capacity: u64,
    lru: Mutex<Lru>,
    stripes: Box<[tokio::sync::Mutex<()>]>,
    hits: AtomicU64,
    reads: AtomicU64,
    read_bytes: AtomicU64,
    evictions: AtomicU64,
    failures: AtomicU64,
}

impl std::fmt::Debug for PrerenderStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrerenderStore")
            .field("capacity", &self.capacity)
            .field("cached_bytes", &self.cached_bytes())
            .finish()
    }
}

impl PrerenderStore {
    /// A store whose cache holds at most `capacity` bytes (0: none).
    pub fn new(capacity: u64) -> Arc<Self> {
        Arc::new(Self {
            capacity,
            lru: Mutex::new(Lru::default()),
            stripes: (0..STRIPES).map(|_| tokio::sync::Mutex::new(())).collect(),
            hits: AtomicU64::new(0),
            reads: AtomicU64::new(0),
            read_bytes: AtomicU64::new(0),
            evictions: AtomicU64::new(0),
            failures: AtomicU64::new(0),
        })
    }

    pub fn capacity(&self) -> u64 {
        self.capacity
    }

    pub fn cached_bytes(&self) -> u64 {
        self.lock().used
    }

    pub fn cached_entries(&self) -> usize {
        self.lock().entries.len()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Lru> {
        self.lru.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The body of entry `id`, from the cache or from its file.
    pub(crate) async fn body(
        &self,
        id: u64,
        source: &FileSource,
        budget: Option<&Arc<NativeBufferBudget>>,
    ) -> Result<Bytes, LoadError> {
        if let Some(bytes) = self.lock().get(id) {
            self.hits.fetch_add(1, Ordering::Relaxed);
            return Ok(bytes);
        }
        let keep = source.len <= self.capacity;
        // Only kept entries are read under the stripe lock: those are the
        // ones a concurrent request can then take from the cache. An entry
        // too large to keep is read by each request for itself.
        let _stripe = if keep {
            let guard = self.stripes[(id as usize) % STRIPES].lock().await;
            if let Some(bytes) = self.lock().get(id) {
                self.hits.fetch_add(1, Ordering::Relaxed);
                return Ok(bytes);
            }
            Some(guard)
        } else {
            None
        };
        // Kept bytes are bounded by the cache; a transient read is reserved
        // in the native buffer budget, when there is one.
        let permit = match (keep, budget) {
            (false, Some(budget)) => Some(
                budget
                    .reserve(BufferKind::Prerender, source.len)
                    .await
                    .ok_or(LoadError::Busy)?,
            ),
            _ => None,
        };
        let file = source.clone();
        let read = tokio::task::spawn_blocking(move || read_exact(&file))
            .await
            .map_err(|e| LoadError::Io(std::io::Error::other(e)))
            .and_then(|r| r);
        let data = match read {
            Ok(data) => data,
            Err(error) => {
                self.failures.fetch_add(1, Ordering::Relaxed);
                return Err(error);
            }
        };
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.read_bytes.fetch_add(source.len, Ordering::Relaxed);
        let bytes = match permit {
            Some(permit) => permit.own(data),
            None => Bytes::from(data),
        };
        if keep && self.capacity > 0 {
            let evicted = self.lock().insert(id, bytes.clone(), self.capacity);
            self.evictions.fetch_add(evicted, Ordering::Relaxed);
        }
        Ok(bytes)
    }

    pub fn to_prometheus(&self) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        let (used, entries) = {
            let lru = self.lock();
            (lru.used, lru.entries.len())
        };
        for (name, kind, help, value) in [
            (
                "nextrs_prerender_cache_capacity_bytes",
                "gauge",
                "Bytes the prerendered-output cache may hold (--prerender-cache).",
                self.capacity,
            ),
            (
                "nextrs_prerender_cache_bytes",
                "gauge",
                "Bytes of prerendered output held in the cache.",
                used,
            ),
            (
                "nextrs_prerender_cache_entries",
                "gauge",
                "Prerendered bodies held in the cache.",
                entries as u64,
            ),
            (
                "nextrs_prerender_cache_hits_total",
                "counter",
                "Prerendered bodies served from the cache.",
                self.hits.load(Ordering::Relaxed),
            ),
            (
                "nextrs_prerender_reads_total",
                "counter",
                "Prerendered bodies read from disk.",
                self.reads.load(Ordering::Relaxed),
            ),
            (
                "nextrs_prerender_read_bytes_total",
                "counter",
                "Bytes of prerendered bodies read from disk.",
                self.read_bytes.load(Ordering::Relaxed),
            ),
            (
                "nextrs_prerender_cache_evictions_total",
                "counter",
                "Prerendered bodies evicted from the cache.",
                self.evictions.load(Ordering::Relaxed),
            ),
            (
                "nextrs_prerender_read_failures_total",
                "counter",
                "Prerendered bodies that could not be read or had changed since startup.",
                self.failures.load(Ordering::Relaxed),
            ),
        ] {
            let _ = writeln!(
                out,
                "# HELP {name} {help}\n# TYPE {name} {kind}\n{name} {value}"
            );
        }
        out
    }
}

/// Reads the whole file, refusing one whose length or modification time
/// differs from what startup recorded (also checked against what was
/// actually read, in case it changes during the read).
fn read_exact(source: &FileSource) -> Result<Vec<u8>, LoadError> {
    let mut file = std::fs::File::open(&source.path).map_err(LoadError::Io)?;
    let metadata = file.metadata().map_err(LoadError::Io)?;
    if metadata.len() != source.len || metadata.modified().ok() != source.modified {
        return Err(LoadError::Changed);
    }
    let len = usize::try_from(source.len).map_err(|_| LoadError::Changed)?;
    let mut data = Vec::with_capacity(len);
    // One byte past the expected length detects a file that grew.
    (&mut file)
        .take(source.len + 1)
        .read_to_end(&mut data)
        .map_err(LoadError::Io)?;
    if data.len() != len {
        return Err(LoadError::Changed);
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(dir: &std::path::Path, name: &str, body: &[u8]) -> FileSource {
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        let metadata = std::fs::metadata(&path).unwrap();
        FileSource {
            path,
            len: metadata.len(),
            modified: metadata.modified().ok(),
        }
    }

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nextrs-prerender-store-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    static NEXT: AtomicU64 = AtomicU64::new(0);

    #[tokio::test]
    async fn reads_once_then_serves_from_the_cache() {
        let dir = tempdir();
        let store = PrerenderStore::new(1024);
        let a = source(&dir, "a.html", b"hello");
        assert_eq!(store.body(1, &a, None).await.unwrap(), "hello");
        assert_eq!(store.body(1, &a, None).await.unwrap(), "hello");
        assert_eq!(store.reads.load(Ordering::Relaxed), 1);
        assert_eq!(store.hits.load(Ordering::Relaxed), 1);
        assert_eq!(store.cached_bytes(), 5);
    }

    #[tokio::test]
    async fn the_cache_is_bounded_and_evicts_the_least_recently_used() {
        let dir = tempdir();
        let store = PrerenderStore::new(10);
        let a = source(&dir, "a", b"aaaa");
        let b = source(&dir, "b", b"bbbb");
        let c = source(&dir, "c", b"cccc");
        store.body(1, &a, None).await.unwrap();
        store.body(2, &b, None).await.unwrap();
        // a used last: b is the oldest when c needs room.
        store.body(1, &a, None).await.unwrap();
        store.body(3, &c, None).await.unwrap();
        assert!(store.cached_bytes() <= 10);
        assert_eq!(store.cached_entries(), 2);
        assert_eq!(store.evictions.load(Ordering::Relaxed), 1);
        let reads = store.reads.load(Ordering::Relaxed);
        store.body(1, &a, None).await.unwrap();
        assert_eq!(store.reads.load(Ordering::Relaxed), reads, "a kept");
        store.body(2, &b, None).await.unwrap();
        assert_eq!(store.reads.load(Ordering::Relaxed), reads + 1, "b evicted");
    }

    #[tokio::test]
    async fn an_entry_larger_than_the_cache_is_served_and_not_kept() {
        let dir = tempdir();
        let store = PrerenderStore::new(4);
        let big = source(&dir, "big", b"0123456789");
        assert_eq!(store.body(1, &big, None).await.unwrap(), "0123456789");
        assert_eq!(store.cached_bytes(), 0);
        // With a budget, its read is reserved while the bytes live.
        let budget = NativeBufferBudget::new(1024 * 1024, std::time::Duration::from_millis(10));
        let bytes = store.body(1, &big, Some(&budget)).await.unwrap();
        assert!(budget.reserved_bytes() > 0);
        drop(bytes);
        assert_eq!(budget.reserved_bytes(), 0);
    }

    #[tokio::test]
    async fn no_cache_reads_every_time() {
        let dir = tempdir();
        let store = PrerenderStore::new(0);
        let a = source(&dir, "a", b"x");
        store.body(1, &a, None).await.unwrap();
        store.body(1, &a, None).await.unwrap();
        assert_eq!(store.reads.load(Ordering::Relaxed), 2);
        assert_eq!(store.cached_bytes(), 0);
    }

    #[tokio::test]
    async fn a_file_changed_after_startup_is_refused() {
        let dir = tempdir();
        let store = PrerenderStore::new(1024);
        let a = source(&dir, "a", b"original");
        // Same length, later modification time.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&a.path, b"replaced").unwrap();
        let file = std::fs::File::options().write(true).open(&a.path).unwrap();
        file.set_modified(SystemTime::now() + std::time::Duration::from_secs(5))
            .unwrap();
        assert!(matches!(
            store.body(1, &a, None).await,
            Err(LoadError::Changed)
        ));
        // Longer.
        let b = source(&dir, "b", b"short");
        let modified = b.modified.unwrap();
        std::fs::write(&b.path, b"much longer").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&b.path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        assert!(matches!(
            store.body(2, &b, None).await,
            Err(LoadError::Changed)
        ));
        // Gone.
        let c = source(&dir, "c", b"here");
        std::fs::remove_file(&c.path).unwrap();
        assert!(matches!(
            store.body(3, &c, None).await,
            Err(LoadError::Io(_))
        ));
        assert_eq!(store.failures.load(Ordering::Relaxed), 3);
        assert_eq!(store.cached_bytes(), 0);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_requests_for_one_entry_read_it_once() {
        let dir = tempdir();
        let store = PrerenderStore::new(1 << 20);
        let a = source(&dir, "a", &vec![7u8; 256 * 1024]);
        let tasks: Vec<_> = (0..32)
            .map(|_| {
                let (store, a) = (store.clone(), a.clone());
                tokio::spawn(async move { store.body(9, &a, None).await.unwrap().len() })
            })
            .collect();
        for task in tasks {
            assert_eq!(task.await.unwrap(), 256 * 1024);
        }
        assert_eq!(store.reads.load(Ordering::Relaxed), 1);
        assert_eq!(store.hits.load(Ordering::Relaxed), 31);
    }
}
