//! Optimized images: a bounded in-memory LRU, and optionally a directory
//! that survives restarts. Separate from the data and response caches (its
//! keys are Next's image cache keys, namespaced by build).
//!
//! Disk entries are one file each, `<dir>/<2 hex>/<64 hex>`: a header
//! with the metadata and a SHA-256 of the body, then the body. They are
//! written to `<dir>/tmp/` and renamed into place, so a reader sees a whole
//! entry or none; an entry that fails its checks is deleted and treated as
//! a miss. Files are opened without following symlinks.

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use bytes::Bytes;
use sha2::{Digest, Sha256};

const MAGIC: &[u8; 8] = b"NXRSIMG1";
const MAX_META: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Entry {
    pub body: Bytes,
    /// `image/webp` etc.; next start's `getContentType(extension)`.
    pub content_type: String,
    pub etag: String,
    pub upstream_etag: String,
    /// `cacheControl.revalidate`: seconds, sent as `max-age`.
    pub max_age: u64,
    /// Unix ms after which the entry is stale.
    pub expire_at_ms: u64,
}

impl Entry {
    fn weight(&self) -> u64 {
        self.body.len() as u64 + 256
    }
}

#[derive(Debug, Clone)]
pub struct ImageCacheOptions {
    /// Persist entries here (created with mode 0700). `None`: memory only.
    pub dir: Option<PathBuf>,
    pub max_memory_bytes: u64,
    pub max_disk_bytes: u64,
    /// Larger entries are served but not cached.
    pub max_entry_bytes: u64,
}

impl Default for ImageCacheOptions {
    fn default() -> Self {
        Self {
            dir: None,
            max_memory_bytes: 64 * 1024 * 1024,
            max_disk_bytes: 1024 * 1024 * 1024,
            max_entry_bytes: 50 * 1024 * 1024,
        }
    }
}

#[derive(Default)]
struct Lru {
    entries: HashMap<String, (std::sync::Arc<Entry>, u64)>,
    order: BTreeMap<u64, String>,
    bytes: u64,
    tick: u64,
}

impl Lru {
    fn touch(&mut self, key: &str) -> Option<std::sync::Arc<Entry>> {
        self.tick += 1;
        let tick = self.tick;
        let (entry, old) = self.entries.get_mut(key)?;
        self.order.remove(old);
        *old = tick;
        self.order.insert(tick, key.to_owned());
        Some(entry.clone())
    }

    fn insert(&mut self, key: String, entry: std::sync::Arc<Entry>, budget: u64) {
        self.remove(&key);
        let weight = entry.weight();
        if weight > budget / 4 {
            return;
        }
        while self.bytes + weight > budget {
            let Some((_, oldest)) = self.order.pop_first() else {
                break;
            };
            if let Some((evicted, _)) = self.entries.remove(&oldest) {
                self.bytes -= evicted.weight();
            }
        }
        self.tick += 1;
        self.order.insert(self.tick, key.clone());
        self.entries.insert(key, (entry, self.tick));
        self.bytes += weight;
    }

    fn remove(&mut self, key: &str) {
        if let Some((entry, tick)) = self.entries.remove(key) {
            self.order.remove(&tick);
            self.bytes -= entry.weight();
        }
    }
}

pub(crate) struct ImageCache {
    memory: Mutex<Lru>,
    disk: Option<Disk>,
    options: ImageCacheOptions,
    /// Namespaces keys: an entry from another build is never served.
    build_id: String,
    pub stats: Stats,
}

#[derive(Default)]
pub(crate) struct Stats {
    pub memory_hits: AtomicU64,
    pub disk_hits: AtomicU64,
    pub misses: AtomicU64,
    pub corrupt: AtomicU64,
    pub write_errors: AtomicU64,
}

impl ImageCache {
    pub(crate) fn open(options: ImageCacheOptions, build_id: &str) -> std::io::Result<Self> {
        let disk = match &options.dir {
            Some(dir) => Some(Disk::open(dir, options.max_disk_bytes)?),
            None => None,
        };
        Ok(Self {
            memory: Mutex::default(),
            disk,
            options,
            build_id: build_id.to_owned(),
            stats: Stats::default(),
        })
    }

    /// The storage name for one of Next's cache keys in this build.
    fn storage_key(&self, key: &str) -> String {
        let mut hash = Sha256::new();
        hash.update(self.build_id.as_bytes());
        hash.update([0]);
        hash.update(key.as_bytes());
        let storage = hex(&hash.finalize());
        #[cfg(feature = "negative-controls")]
        let storage = super::controls::cache_key(storage);
        storage
    }

    pub(crate) async fn get(&self, key: &str) -> Option<std::sync::Arc<Entry>> {
        let storage = self.storage_key(key);
        if let Some(entry) = self.memory.lock().expect("cache lock").touch(&storage) {
            self.stats.memory_hits.fetch_add(1, Ordering::Relaxed);
            return Some(entry);
        }
        let Some(disk) = &self.disk else {
            self.stats.misses.fetch_add(1, Ordering::Relaxed);
            return None;
        };
        match disk.read(&storage, self.options.max_entry_bytes).await {
            Read_::Hit(entry) => {
                self.stats.disk_hits.fetch_add(1, Ordering::Relaxed);
                let entry = std::sync::Arc::new(entry);
                self.memory.lock().expect("cache lock").insert(
                    storage,
                    entry.clone(),
                    self.options.max_memory_bytes,
                );
                Some(entry)
            }
            Read_::Corrupt => {
                self.stats.corrupt.fetch_add(1, Ordering::Relaxed);
                self.stats.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
            Read_::Miss => {
                self.stats.misses.fetch_add(1, Ordering::Relaxed);
                None
            }
        }
    }

    pub(crate) async fn set(&self, key: &str, entry: std::sync::Arc<Entry>) {
        if entry.body.is_empty() || entry.body.len() as u64 > self.options.max_entry_bytes {
            return;
        }
        let storage = self.storage_key(key);
        self.memory.lock().expect("cache lock").insert(
            storage.clone(),
            entry.clone(),
            self.options.max_memory_bytes,
        );
        if let Some(disk) = &self.disk
            && let Err(error) = disk.write(&storage, &entry).await
        {
            self.stats.write_errors.fetch_add(1, Ordering::Relaxed);
            tracing::warn!(%error, "image cache: entry not persisted");
        }
    }

    pub(crate) fn memory_bytes(&self) -> u64 {
        self.memory.lock().expect("cache lock").bytes
    }

    pub(crate) fn disk_bytes(&self) -> u64 {
        self.disk
            .as_ref()
            .map_or(0, |d| d.index.lock().expect("index lock").bytes)
    }
}

enum Read_ {
    Hit(Entry),
    Miss,
    Corrupt,
}

#[derive(Default)]
struct Index {
    /// storage key -> (bytes on disk, last use tick)
    files: HashMap<String, (u64, u64)>,
    order: BTreeMap<u64, String>,
    bytes: u64,
    tick: u64,
}

impl Index {
    fn touch(&mut self, key: &str) {
        self.tick += 1;
        let tick = self.tick;
        if let Some((_, old)) = self.files.get_mut(key) {
            self.order.remove(old);
            *old = tick;
            self.order.insert(tick, key.to_owned());
        }
    }

    fn insert(&mut self, key: &str, size: u64) {
        self.remove(key);
        self.tick += 1;
        self.files.insert(key.to_owned(), (size, self.tick));
        self.order.insert(self.tick, key.to_owned());
        self.bytes += size;
    }

    fn remove(&mut self, key: &str) -> bool {
        match self.files.remove(key) {
            Some((size, tick)) => {
                self.order.remove(&tick);
                self.bytes -= size;
                true
            }
            None => false,
        }
    }
}

struct Disk {
    dir: PathBuf,
    max_bytes: u64,
    index: Mutex<Index>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn valid_name(name: &str) -> bool {
    name.len() == 64 && name.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn open_nofollow(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc_o_nofollow())
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::other("not a regular file"));
    }
    Ok(file)
}

const fn libc_o_nofollow() -> i32 {
    libc::O_NOFOLLOW
}

impl Disk {
    fn open(dir: &Path, max_bytes: u64) -> std::io::Result<Self> {
        use std::os::unix::fs::DirBuilderExt;
        match std::fs::symlink_metadata(dir) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(std::io::Error::other(format!(
                    "{} is a symlink",
                    dir.display()
                )));
            }
            Ok(meta) if !meta.is_dir() => {
                return Err(std::io::Error::other(format!(
                    "{} is not a directory",
                    dir.display()
                )));
            }
            Ok(_) => {}
            Err(_) => std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(dir)?,
        }
        let tmp = dir.join("tmp");
        // Leftovers of writes a crash interrupted.
        if let Ok(entries) = std::fs::read_dir(&tmp) {
            for entry in entries.flatten() {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&tmp)?;
        let mut found: Vec<(std::time::SystemTime, String, u64)> = Vec::new();
        for shard in std::fs::read_dir(dir)?.flatten() {
            let Ok(kind) = shard.file_type() else {
                continue;
            };
            let name = shard.file_name();
            let Some(name) = name.to_str() else { continue };
            if !kind.is_dir() || name.len() != 2 {
                continue;
            }
            for file in std::fs::read_dir(shard.path())?.flatten() {
                let Ok(meta) = file.metadata() else { continue };
                let fname = file.file_name();
                match fname.to_str() {
                    Some(fname)
                        if meta.is_file() && valid_name(fname) && fname.starts_with(name) =>
                    {
                        found.push((
                            meta.modified().unwrap_or(std::time::UNIX_EPOCH),
                            fname.to_owned(),
                            meta.len(),
                        ));
                    }
                    _ => {
                        let _ = std::fs::remove_file(file.path());
                    }
                }
            }
        }
        found.sort();
        let disk = Self {
            dir: dir.to_owned(),
            max_bytes,
            index: Mutex::default(),
        };
        {
            let mut index = disk.index.lock().expect("index lock");
            for (_, name, size) in found {
                index.insert(&name, size);
            }
        }
        disk.evict(0);
        Ok(disk)
    }

    fn path(&self, key: &str) -> PathBuf {
        self.dir.join(&key[..2]).join(key)
    }

    /// Removes least recently used files until `incoming` more bytes fit.
    fn evict(&self, incoming: u64) {
        loop {
            let victim = {
                let mut index = self.index.lock().expect("index lock");
                if index.bytes + incoming <= self.max_bytes {
                    return;
                }
                let Some((_, key)) = index.order.pop_first() else {
                    return;
                };
                if let Some((size, _)) = index.files.remove(&key) {
                    index.bytes -= size;
                }
                key
            };
            let _ = std::fs::remove_file(self.path(&victim));
        }
    }

    async fn read(&self, key: &str, max_entry: u64) -> Read_ {
        if !self
            .index
            .lock()
            .expect("index lock")
            .files
            .contains_key(key)
        {
            return Read_::Miss;
        }
        let path = self.path(key);
        let key_owned = key.to_owned();
        let parsed = tokio::task::spawn_blocking(move || -> Result<Option<Entry>, ()> {
            let mut file = match open_nofollow(&path) {
                Ok(file) => file,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(_) => {
                    let _ = std::fs::remove_file(&path);
                    return Err(());
                }
            };
            let len = file.metadata().map_err(|_| ())?.len();
            if len > max_entry + MAX_META as u64 + 64 {
                let _ = std::fs::remove_file(&path);
                return Err(());
            }
            let mut raw = Vec::with_capacity(len as usize);
            file.read_to_end(&mut raw).map_err(|_| ())?;
            match decode(&key_owned, &raw) {
                Some(entry) => Ok(Some(entry)),
                None => {
                    let _ = std::fs::remove_file(&path);
                    Err(())
                }
            }
        })
        .await
        .unwrap_or(Err(()));
        match parsed {
            Ok(Some(entry)) => {
                self.index.lock().expect("index lock").touch(key);
                Read_::Hit(entry)
            }
            Ok(None) => {
                self.index.lock().expect("index lock").remove(key);
                Read_::Miss
            }
            Err(()) => {
                self.index.lock().expect("index lock").remove(key);
                Read_::Corrupt
            }
        }
    }

    async fn write(&self, key: &str, entry: &Entry) -> std::io::Result<()> {
        let encoded = encode(key, entry);
        let size = encoded.len() as u64;
        if size > self.max_bytes {
            return Ok(());
        }
        self.evict(size);
        let dir = self.dir.clone();
        let target = self.path(key);
        let key_owned = key.to_owned();
        tokio::task::spawn_blocking(move || -> std::io::Result<()> {
            use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
            let shard = target.parent().expect("sharded path");
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(shard)?;
            let nonce: u64 = rand_nonce();
            let tmp = dir.join("tmp").join(format!("{key_owned}.{nonce:016x}"));
            let result = (|| {
                let mut file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .custom_flags(libc_o_nofollow())
                    .open(&tmp)?;
                file.write_all(&encoded)?;
                drop(file);
                std::fs::rename(&tmp, &target)
            })();
            if result.is_err() {
                let _ = std::fs::remove_file(&tmp);
            }
            result
        })
        .await
        .map_err(std::io::Error::other)??;
        self.index.lock().expect("index lock").insert(key, size);
        Ok(())
    }
}

fn rand_nonce() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    let mut h = std::collections::hash_map::RandomState::new().build_hasher();
    h.write_u64(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64),
    );
    h.finish()
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Meta {
    key: String,
    content_type: String,
    etag: String,
    upstream_etag: String,
    max_age: u64,
    expire_at_ms: u64,
    body_len: u64,
    body_sha256: String,
}

fn encode(key: &str, entry: &Entry) -> Vec<u8> {
    let meta = Meta {
        key: key.to_owned(),
        content_type: entry.content_type.clone(),
        etag: entry.etag.clone(),
        upstream_etag: entry.upstream_etag.clone(),
        max_age: entry.max_age,
        expire_at_ms: entry.expire_at_ms,
        body_len: entry.body.len() as u64,
        body_sha256: hex(&Sha256::digest(&entry.body)),
    };
    let meta = serde_json::to_vec(&meta).expect("serializable");
    let mut out = Vec::with_capacity(12 + meta.len() + entry.body.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(meta.len() as u32).to_le_bytes());
    out.extend_from_slice(&meta);
    out.extend_from_slice(&entry.body);
    out
}

fn decode(key: &str, raw: &[u8]) -> Option<Entry> {
    if raw.get(..8)? != MAGIC {
        return None;
    }
    let meta_len = u32::from_le_bytes(raw.get(8..12)?.try_into().ok()?) as usize;
    if meta_len > MAX_META {
        return None;
    }
    let meta: Meta = serde_json::from_slice(raw.get(12..12 + meta_len)?).ok()?;
    let body = raw.get(12 + meta_len..)?;
    if meta.key != key
        || meta.body_len != body.len() as u64
        || body.is_empty()
        || hex(&Sha256::digest(body)) != meta.body_sha256
        || !meta.content_type.starts_with("image/")
    {
        return None;
    }
    Some(Entry {
        body: Bytes::copy_from_slice(body),
        content_type: meta.content_type,
        etag: meta.etag,
        upstream_etag: meta.upstream_etag,
        max_age: meta.max_age,
        expire_at_ms: meta.expire_at_ms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn entry(body: &[u8]) -> Arc<Entry> {
        Arc::new(Entry {
            body: Bytes::copy_from_slice(body),
            content_type: "image/webp".into(),
            etag: "e".into(),
            upstream_etag: "u".into(),
            max_age: 60,
            expire_at_ms: 1,
        })
    }

    fn tempdir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("nextrs-image-cache-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn options(dir: Option<PathBuf>) -> ImageCacheOptions {
        ImageCacheOptions {
            dir,
            max_memory_bytes: 64 * 1024,
            max_disk_bytes: 8 * 1024,
            max_entry_bytes: 4 * 1024,
        }
    }

    #[tokio::test]
    async fn memory_lru_is_bounded() {
        let cache = ImageCache::open(options(None), "b").unwrap();
        for i in 0..100 {
            cache.set(&format!("k{i}"), entry(&[i as u8; 3000])).await;
        }
        assert!(cache.memory_bytes() <= 64 * 1024);
        assert!(cache.get("k99").await.is_some());
        assert!(cache.get("k0").await.is_none());
    }

    #[tokio::test]
    async fn persists_across_restarts_per_build() {
        let dir = tempdir("persist");
        let cache = ImageCache::open(options(Some(dir.clone())), "build-1").unwrap();
        cache.set("k", entry(b"abc")).await;
        drop(cache);
        let cache = ImageCache::open(options(Some(dir.clone())), "build-1").unwrap();
        assert_eq!(cache.get("k").await.unwrap().body.as_ref(), b"abc");
        let other = ImageCache::open(options(Some(dir.clone())), "build-2").unwrap();
        assert!(other.get("k").await.is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn corrupt_entries_are_deleted() {
        let dir = tempdir("corrupt");
        let cache = ImageCache::open(options(Some(dir.clone())), "b").unwrap();
        cache.set("k", entry(b"abcdef")).await;
        let storage = cache.storage_key("k");
        let path = dir.join(&storage[..2]).join(&storage);
        let mut raw = std::fs::read(&path).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 1;
        std::fs::write(&path, raw).unwrap();
        let cache = ImageCache::open(options(Some(dir.clone())), "b").unwrap();
        assert!(cache.get("k").await.is_none());
        assert!(!path.exists());
        assert_eq!(cache.stats.corrupt.load(Ordering::Relaxed), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn symlinked_entries_are_not_followed() {
        let dir = tempdir("symlink");
        let cache = ImageCache::open(options(Some(dir.clone())), "b").unwrap();
        cache.set("k", entry(b"abc")).await;
        let storage = cache.storage_key("k");
        let path = dir.join(&storage[..2]).join(&storage);
        let secret = dir.join("secret");
        std::fs::write(&secret, encode(&storage, &entry(b"leaked"))).unwrap();
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&secret, &path).unwrap();
        let cache = ImageCache::open(options(Some(dir.clone())), "b").unwrap();
        assert!(cache.get("k").await.is_none());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn disk_quota_evicts_oldest() {
        let dir = tempdir("quota");
        let cache = ImageCache::open(
            ImageCacheOptions {
                max_memory_bytes: 4096,
                ..options(Some(dir.clone()))
            },
            "b",
        )
        .unwrap();
        for i in 0..10 {
            cache.set(&format!("k{i}"), entry(&[i as u8; 2000])).await;
        }
        assert!(cache.disk_bytes() <= 8 * 1024);
        let _ = std::fs::remove_dir_all(dir);
    }
}
