//! The data cache shared by every isolate: Next.js `fetch` caching and
//! `unstable_cache`, stored in Rust and optionally persisted to disk.
//!
//! JavaScript reaches it through a `CacheHandler` (Next's documented
//! interface, `js/03_server.js`) and four ops; it passes cache keys, tags and
//! opaque values, never paths. Entries live in memory up to a byte budget
//! and, with a directory, in files named by the SHA-256 of their key under a
//! namespace derived from the build ID and the JavaScript environment, so a
//! different build or environment never reads another's entries.
//!
//! Tag semantics follow Next's own `FileSystemCache` (tags manifest with
//! `stale`/`expired` times): a tag expired after an entry was written makes
//! it a miss; a tag marked stale after it makes it stale (served, then
//! refreshed by Next in the background).
//!
//! Files are written to a temporary name and renamed into place, carry a
//! checksum, and are verified when read: a torn or corrupted file is a miss
//! and is removed. Disk access runs on blocking threads, never on a V8
//! thread.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::metrics::{self, Counters};

/// Bumped whenever the file format or key derivation changes.
const FORMAT_VERSION: u8 = 1;
const MAGIC: &[u8; 4] = b"NXRC";
/// Revalidated tags remembered (oldest dropped beyond this).
const MAX_TAGS: usize = 10_000;
/// Next's keys are 64-character hashes; anything far larger is refused, so
/// keys cannot outgrow the byte budgets (which count values).
const MAX_KEY_BYTES: usize = 4096;
/// Next allows 128 tags of 256 characters per entry.
const MAX_ENTRY_TAGS: usize = 256;
const MAX_TAG_BYTES: usize = 1024;

#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Persist entries here (created if missing, mode 0700). `None` keeps
    /// the cache in memory only, shared by the isolates of this process.
    pub dir: Option<PathBuf>,
    /// Entry values held in memory. With a directory, values evicted from
    /// memory stay on disk.
    pub max_memory_bytes: u64,
    /// Entry files on disk; least recently used entries are removed beyond.
    pub max_disk_bytes: u64,
    /// Larger values are not stored (Next's own limit is 2 MB).
    pub max_entry_bytes: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            dir: None,
            max_memory_bytes: 50 * 1024 * 1024,
            max_disk_bytes: 1024 * 1024 * 1024,
            max_entry_bytes: 2 * 1024 * 1024,
        }
    }
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
struct TagState {
    /// Entries written before this are stale.
    stale: Option<u64>,
    /// Entries written before this are gone, once this time has passed.
    expired: Option<u64>,
}

struct Entry {
    /// `None` when evicted from memory; then the entry is on disk.
    value: Option<Bytes>,
    size: u64,
    last_modified: u64,
    tags: Vec<String>,
    /// Which `set` produced this entry (orders concurrent disk writes).
    generation: u64,
    on_disk: bool,
    lru: u64,
}

#[derive(Default)]
struct State {
    entries: HashMap<String, Entry>,
    /// Recency order: lru stamp -> key.
    lru: BTreeMap<u64, String>,
    next_stamp: u64,
    next_generation: u64,
    memory_bytes: u64,
    disk_bytes: u64,
    tags: HashMap<String, TagState>,
}

impl State {
    fn touch(&mut self, key: &str) {
        let stamp = self.next_stamp;
        self.next_stamp += 1;
        if let Some(entry) = self.entries.get_mut(key) {
            self.lru.remove(&entry.lru);
            entry.lru = stamp;
            self.lru.insert(stamp, key.to_owned());
        }
    }

    fn remove(&mut self, key: &str) -> Option<Entry> {
        let entry = self.entries.remove(key)?;
        self.lru.remove(&entry.lru);
        if entry.value.is_some() {
            self.memory_bytes -= entry.size;
        }
        if entry.on_disk {
            self.disk_bytes -= entry.size;
        }
        Some(entry)
    }

    /// Fresh, stale, or gone, judged by every tag that applies.
    fn judge<'a>(
        &self,
        last_modified: u64,
        tags: impl Iterator<Item = &'a String>,
        now: u64,
    ) -> Freshness {
        let mut stale = false;
        for tag in tags {
            let Some(state) = self.tags.get(tag) else {
                continue;
            };
            if state
                .expired
                .is_some_and(|at| at <= now && at > last_modified)
            {
                return Freshness::Expired;
            }
            if state.stale.is_some_and(|at| at > last_modified) {
                stale = true;
            }
        }
        if stale {
            Freshness::Stale
        } else {
            Freshness::Fresh
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Freshness {
    Fresh,
    Stale,
    Expired,
}

/// A cache hit.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Hit {
    pub value: String,
    /// Milliseconds since the epoch.
    pub last_modified: f64,
    /// A tag was marked stale after the entry was written.
    pub stale: bool,
}

/// What `lookup` found without touching the disk.
pub(crate) enum Lookup {
    Hit(Hit),
    Miss,
    /// The entry is only on disk: read it with `load` (blocking).
    OnDisk {
        path: PathBuf,
        generation: u64,
    },
}

/// A file to write for a `set` (blocking, outside the lock).
pub(crate) struct PendingWrite {
    key: String,
    generation: u64,
    path: PathBuf,
    contents: Vec<u8>,
}

struct Disk {
    entries: PathBuf,
    tmp: PathBuf,
    tags_file: PathBuf,
}

pub(crate) struct CacheStore {
    state: Mutex<State>,
    disk: Option<Disk>,
    config: CacheConfig,
    counters: Arc<Counters>,
    /// Serializes file writes so the newest generation wins on disk.
    write_lock: Mutex<()>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileHeader {
    key: String,
    last_modified: u64,
    tags: Vec<String>,
    sha256: String,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn file_name(key: &str) -> String {
    hex(&Sha256::digest(key.as_bytes()))
}

/// The namespace directory name: build ID and environment, hashed.
pub(crate) fn namespace(build_id: &str, env: &BTreeMap<String, String>) -> String {
    let mut hash = Sha256::new();
    hash.update([FORMAT_VERSION]);
    hash.update(build_id.as_bytes());
    for (name, value) in env {
        hash.update([0]);
        hash.update(name.as_bytes());
        hash.update([0]);
        hash.update(value.as_bytes());
    }
    format!("v{FORMAT_VERSION}-{}", &hex(&hash.finalize())[..24])
}

fn encode(key: &str, value: &[u8], last_modified: u64, tags: &[String]) -> Vec<u8> {
    let header = serde_json::to_vec(&FileHeader {
        key: key.to_owned(),
        last_modified,
        tags: tags.to_vec(),
        sha256: hex(&Sha256::digest(value)),
    })
    .expect("header serializes");
    let mut out = Vec::with_capacity(9 + header.len() + value.len());
    out.extend_from_slice(MAGIC);
    out.push(FORMAT_VERSION);
    out.extend_from_slice(&(header.len() as u32).to_le_bytes());
    out.extend_from_slice(&header);
    out.extend_from_slice(value);
    out
}

/// Reads a file's header; with `with_value`, also its value, verified
/// against the header's checksum.
fn decode(path: &Path, with_value: bool) -> io::Result<(FileHeader, Option<Vec<u8>>)> {
    let invalid = |what: &str| io::Error::new(io::ErrorKind::InvalidData, what.to_owned());
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() {
        return Err(invalid("not a regular file"));
    }
    let mut file = fs::File::open(path)?;
    let mut prefix = [0u8; 9];
    file.read_exact(&mut prefix)?;
    if &prefix[..4] != MAGIC || prefix[4] != FORMAT_VERSION {
        return Err(invalid("bad magic or version"));
    }
    let header_len = u32::from_le_bytes(prefix[5..9].try_into().unwrap()) as u64;
    if header_len > 1024 * 1024 || 9 + header_len > metadata.len() {
        return Err(invalid("bad header length"));
    }
    let mut header = vec![0u8; header_len as usize];
    file.read_exact(&mut header)?;
    let header: FileHeader = serde_json::from_slice(&header).map_err(|_| invalid("bad header"))?;
    if !with_value {
        return Ok((header, None));
    }
    let mut value = Vec::with_capacity((metadata.len() - 9 - header_len) as usize);
    file.read_to_end(&mut value)?;
    if hex(&Sha256::digest(&value)) != header.sha256 {
        return Err(invalid("checksum mismatch"));
    }
    Ok((header, Some(value)))
}

/// Creates a directory owned by this process (0700), refusing symlinks.
fn private_dir(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is a symlink", path.display()),
        )),
        Ok(metadata) if !metadata.is_dir() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not a directory", path.display()),
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            use std::os::unix::fs::DirBuilderExt;
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(path)
        }
        Err(error) => Err(error),
    }
}

/// Writes `contents` to `path` atomically: a temporary file in `tmp`, then
/// a rename (same filesystem).
fn write_atomic(tmp: &Path, path: &Path, contents: &[u8]) -> io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut nonce = [0u8; 8];
    getrandom::fill(&mut nonce).map_err(|e| io::Error::other(e.to_string()))?;
    let temp = tmp.join(format!("{}.tmp", hex(&nonce)));
    let result = (|| {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        file.write_all(contents)?;
        file.flush()?;
        fs::rename(&temp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}

impl CacheStore {
    /// Opens the store; with a directory, loads the namespace's index
    /// (headers only), removes leftover temporary files and the namespaces
    /// of other builds or environments.
    pub fn open(
        config: CacheConfig,
        namespace: &str,
        counters: Arc<Counters>,
    ) -> Result<Self, String> {
        let mut state = State::default();
        let disk = match &config.dir {
            None => None,
            Some(root) => {
                let fail = |e: io::Error| format!("cache directory {}: {e}", root.display());
                private_dir(root).map_err(fail)?;
                for other in fs::read_dir(root).map_err(fail)?.flatten() {
                    let name = other.file_name();
                    let name = name.to_string_lossy();
                    // Only directories this cache created.
                    if name.starts_with('v') && name.contains('-') && name != namespace {
                        let path = other.path();
                        if fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir()) {
                            let _ = fs::remove_dir_all(&path);
                        }
                    }
                }
                let ns = root.join(namespace);
                let disk = Disk {
                    entries: ns.join("entries"),
                    tmp: ns.join("tmp"),
                    tags_file: ns.join("tags.json"),
                };
                for dir in [&ns, &disk.entries, &disk.tmp] {
                    private_dir(dir).map_err(fail)?;
                }
                for leftover in fs::read_dir(&disk.tmp).map_err(fail)?.flatten() {
                    let _ = fs::remove_file(leftover.path());
                }
                if let Ok(text) = fs::read(&disk.tags_file) {
                    match serde_json::from_slice::<HashMap<String, TagState>>(&text) {
                        Ok(tags) => state.tags = tags,
                        Err(_) => {
                            metrics::add(&counters.cache_corrupt, 1);
                            let _ = fs::remove_file(&disk.tags_file);
                        }
                    }
                }
                let mut found: Vec<(u64, String, u64, Vec<String>)> = Vec::new();
                for file in fs::read_dir(&disk.entries).map_err(fail)?.flatten() {
                    let path = file.path();
                    match decode(&path, false) {
                        Ok((header, None))
                            if path
                                .file_name()
                                .is_some_and(|n| n.to_string_lossy() == file_name(&header.key)) =>
                        {
                            let size = file.metadata().map_or(0, |m| m.len());
                            found.push((header.last_modified, header.key, size, header.tags));
                        }
                        _ => {
                            metrics::add(&counters.cache_corrupt, 1);
                            let _ = fs::remove_file(&path);
                        }
                    }
                }
                // Oldest first, so recency follows modification time.
                found.sort_by_key(|(modified, ..)| *modified);
                for (last_modified, key, size, tags) in found {
                    let stamp = state.next_stamp;
                    state.next_stamp += 1;
                    state.lru.insert(stamp, key.clone());
                    state.disk_bytes += size;
                    state.entries.insert(
                        key,
                        Entry {
                            value: None,
                            size,
                            last_modified,
                            tags,
                            generation: 0,
                            on_disk: true,
                            lru: stamp,
                        },
                    );
                }
                Some(disk)
            }
        };
        state.next_generation = 1;
        let store = Self {
            state: Mutex::new(state),
            disk,
            config,
            counters,
            write_lock: Mutex::new(()),
        };
        // An index loaded from disk may exceed a smaller limit.
        let removed = store.enforce_limits(&mut store.lock());
        store.delete_files(removed);
        Ok(store)
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn entries(&self) -> (u64, u64, u64) {
        let state = self.lock();
        (
            state.entries.len() as u64,
            state.memory_bytes,
            state.disk_bytes,
        )
    }

    /// Looks a key up in memory. Tags are the caller's (`tags` and
    /// `softTags`) in addition to those stored with the entry.
    pub fn lookup(&self, key: &str, tags: &[String]) -> Lookup {
        if key.len() > MAX_KEY_BYTES {
            metrics::add(&self.counters.cache_misses, 1);
            return Lookup::Miss;
        }
        let now = now_ms();
        let mut state = self.lock();
        let Some(entry) = state.entries.get(key) else {
            drop(state);
            metrics::add(&self.counters.cache_misses, 1);
            return Lookup::Miss;
        };
        let freshness = state.judge(entry.last_modified, entry.tags.iter().chain(tags), now);
        if freshness == Freshness::Expired {
            drop(state);
            metrics::add(&self.counters.cache_misses, 1);
            return Lookup::Miss;
        }
        match &entry.value {
            Some(value) => {
                let hit = Hit {
                    value: String::from_utf8_lossy(value).into_owned(),
                    last_modified: entry.last_modified as f64,
                    stale: freshness == Freshness::Stale,
                };
                state.touch(key);
                drop(state);
                metrics::add(&self.counters.cache_hits, 1);
                Lookup::Hit(hit)
            }
            None => {
                let generation = entry.generation;
                let path = self
                    .disk
                    .as_ref()
                    .expect("values leave memory only when on disk")
                    .entries
                    .join(file_name(key));
                Lookup::OnDisk { path, generation }
            }
        }
    }

    /// Reads an entry that was only on disk (blocking), keeps its value in
    /// memory, and judges it again.
    pub fn load(&self, key: &str, tags: &[String], path: &Path, generation: u64) -> Option<Hit> {
        let read = decode(path, true);
        let mut state = self.lock();
        let current = state.entries.get(key).map(|e| e.generation);
        let value = match read {
            Ok((header, Some(value))) if header.key == key => value,
            Ok(_) | Err(_) => {
                // Corrupted or replaced underneath us: drop it (a miss).
                let removed = if current == Some(generation) {
                    state.remove(key)
                } else {
                    None
                };
                drop(state);
                if removed.is_some() {
                    metrics::add(&self.counters.cache_corrupt, 1);
                    let _ = fs::remove_file(path);
                }
                metrics::add(&self.counters.cache_misses, 1);
                return None;
            }
        };
        if current != Some(generation) {
            // A newer set replaced it while we read: ask again.
            drop(state);
            return match self.lookup(key, tags) {
                Lookup::Hit(hit) => Some(hit),
                _ => None,
            };
        }
        let value = Bytes::from(value);
        let entry = state.entries.get_mut(key).expect("checked above");
        let hit_tags = entry.tags.clone();
        let last_modified = entry.last_modified;
        if entry.value.is_none() {
            entry.value = Some(value.clone());
            let size = entry.size;
            state.memory_bytes += size;
        }
        state.touch(key);
        let freshness = state.judge(last_modified, hit_tags.iter().chain(tags), now_ms());
        let removed = self.enforce_limits(&mut state);
        drop(state);
        self.delete_files(removed);
        if freshness == Freshness::Expired {
            metrics::add(&self.counters.cache_misses, 1);
            return None;
        }
        metrics::add(&self.counters.cache_hits, 1);
        Some(Hit {
            value: String::from_utf8_lossy(&value).into_owned(),
            last_modified: last_modified as f64,
            stale: freshness == Freshness::Stale,
        })
    }

    /// Stores a value (visible to every isolate at once). Returns the file
    /// write to perform, if the cache persists.
    pub fn set(&self, key: &str, value: Bytes, tags: Vec<String>) -> Option<PendingWrite> {
        let size = value.len() as u64;
        let oversized_tags =
            tags.len() > MAX_ENTRY_TAGS || tags.iter().any(|t| t.len() > MAX_TAG_BYTES);
        if size > self.config.max_entry_bytes || key.len() > MAX_KEY_BYTES || oversized_tags {
            metrics::add(&self.counters.cache_too_large, 1);
            // Whatever was there is outdated now.
            self.delete(key);
            return None;
        }
        let last_modified = now_ms();
        let mut state = self.lock();
        state.remove(key);
        let generation = state.next_generation;
        state.next_generation += 1;
        let stamp = state.next_stamp;
        state.next_stamp += 1;
        let write = self.disk.as_ref().map(|disk| PendingWrite {
            key: key.to_owned(),
            generation,
            path: disk.entries.join(file_name(key)),
            contents: encode(key, &value, last_modified, &tags),
        });
        state.memory_bytes += size;
        state.lru.insert(stamp, key.to_owned());
        state.entries.insert(
            key.to_owned(),
            Entry {
                value: Some(value),
                size,
                last_modified,
                tags,
                generation,
                on_disk: false,
                lru: stamp,
            },
        );
        metrics::add(&self.counters.cache_sets, 1);
        let removed = self.enforce_limits(&mut state);
        drop(state);
        self.delete_files(removed);
        write
    }

    /// Writes a pending entry file (blocking). Skipped if a newer `set`
    /// replaced the entry meanwhile.
    pub fn persist(&self, write: PendingWrite) {
        let Some(disk) = &self.disk else {
            return;
        };
        let _serial = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        if self.lock().entries.get(&write.key).map(|e| e.generation) != Some(write.generation) {
            return;
        }
        if let Err(error) = write_atomic(&disk.tmp, &write.path, &write.contents) {
            metrics::add(&self.counters.cache_io_errors, 1);
            tracing::warn!(target: "nextrs::cache", %error, "cannot persist a cache entry");
            return;
        }
        let mut state = self.lock();
        let size = write.contents.len() as u64;
        match state.entries.get_mut(&write.key) {
            Some(entry) if entry.generation == write.generation => {
                // The file is the value plus a small header; account the file.
                if entry.value.is_some() {
                    state.memory_bytes -= entry.size;
                }
                let entry = state.entries.get_mut(&write.key).expect("present");
                entry.size = size;
                entry.on_disk = true;
                if entry.value.is_some() {
                    state.memory_bytes += size;
                }
                state.disk_bytes += size;
                let removed = self.enforce_limits(&mut state);
                drop(state);
                self.delete_files(removed);
            }
            _ => {
                // Replaced or removed while writing.
                drop(state);
                let _ = fs::remove_file(&write.path);
            }
        }
    }

    pub fn delete(&self, key: &str) {
        let removed = self.lock().remove(key);
        if removed.is_some_and(|entry| entry.on_disk) {
            self.delete_files(vec![key.to_owned()]);
        }
    }

    /// Next's `revalidateTag(tags, durations)`: without durations the tags
    /// expire now; with them they are stale now and expire after
    /// `expire_seconds` (if given). Returns the tags file contents to
    /// persist.
    pub fn revalidate(
        &self,
        tags: &[String],
        expire_seconds: Option<f64>,
        with_durations: bool,
    ) -> Option<Vec<u8>> {
        let now = now_ms();
        let mut state = self.lock();
        for tag in tags {
            let entry = state.tags.entry(tag.clone()).or_default();
            if with_durations {
                entry.stale = Some(now);
                if let Some(expire) = expire_seconds {
                    entry.expired = Some(now + (expire.max(0.0) * 1000.0) as u64);
                }
            } else {
                entry.expired = Some(now);
            }
        }
        if state.tags.len() > MAX_TAGS {
            // Forget the tags whose last event is oldest.
            let mut by_age: Vec<(u64, String)> = state
                .tags
                .iter()
                .map(|(tag, s)| (s.stale.max(s.expired).unwrap_or(0), tag.clone()))
                .collect();
            by_age.sort();
            for (_, tag) in by_age.into_iter().take(state.tags.len() - MAX_TAGS) {
                state.tags.remove(&tag);
            }
        }
        metrics::add(&self.counters.cache_revalidations, tags.len() as u64);
        self.disk
            .as_ref()
            .map(|_| serde_json::to_vec(&state.tags).expect("tags serialize"))
    }

    /// Writes the tags file (blocking).
    pub fn persist_tags(&self, contents: Vec<u8>) {
        let Some(disk) = &self.disk else {
            return;
        };
        let _serial = self.write_lock.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(error) = write_atomic(&disk.tmp, &disk.tags_file, &contents) {
            metrics::add(&self.counters.cache_io_errors, 1);
            tracing::warn!(target: "nextrs::cache", %error, "cannot persist cache tags");
        }
    }

    /// Evicts values from memory and entries from disk beyond the limits.
    /// Returns keys whose files must be deleted.
    fn enforce_limits(&self, state: &mut State) -> Vec<String> {
        let mut delete = Vec::new();
        let mut stamps: Vec<u64> = Vec::new();
        if state.memory_bytes > self.config.max_memory_bytes
            || state.disk_bytes > self.config.max_disk_bytes
        {
            stamps.extend(state.lru.keys().copied());
        }
        for stamp in stamps {
            let over_memory = state.memory_bytes > self.config.max_memory_bytes;
            let over_disk = state.disk_bytes > self.config.max_disk_bytes;
            if !over_memory && !over_disk {
                break;
            }
            let Some(key) = state.lru.get(&stamp).cloned() else {
                continue;
            };
            let entry = state.entries.get_mut(&key).expect("lru and entries agree");
            if over_disk && entry.on_disk || over_memory && !entry.on_disk {
                // Gone entirely.
                let entry = state.remove(&key).expect("present");
                metrics::add(&self.counters.cache_evictions, 1);
                if entry.on_disk {
                    delete.push(key);
                }
            } else if over_memory && entry.on_disk && entry.value.take().is_some() {
                // Still on disk; only the in-memory copy goes.
                let size = entry.size;
                state.memory_bytes -= size;
            }
        }
        delete
    }

    fn delete_files(&self, keys: Vec<String>) {
        let Some(disk) = &self.disk else {
            return;
        };
        for key in keys {
            let _ = fs::remove_file(disk.entries.join(file_name(&key)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "nextrs-cache-{name}-{}-{}",
            std::process::id(),
            now_ms()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn store(dir: Option<PathBuf>, ns: &str, max_memory: u64, max_disk: u64) -> CacheStore {
        CacheStore::open(
            CacheConfig {
                dir,
                max_memory_bytes: max_memory,
                max_disk_bytes: max_disk,
                max_entry_bytes: 1024 * 1024,
            },
            ns,
            Arc::new(Counters::default()),
        )
        .unwrap()
    }

    fn set(store: &CacheStore, key: &str, value: &str, tags: &[&str]) {
        let write = store.set(
            key,
            Bytes::from(value.to_owned()),
            tags.iter().map(|t| (*t).to_owned()).collect(),
        );
        if let Some(write) = write {
            store.persist(write);
        }
    }

    fn get(store: &CacheStore, key: &str, tags: &[&str]) -> Option<Hit> {
        let tags: Vec<String> = tags.iter().map(|t| (*t).to_owned()).collect();
        match store.lookup(key, &tags) {
            Lookup::Hit(hit) => Some(hit),
            Lookup::Miss => None,
            Lookup::OnDisk { path, generation } => store.load(key, &tags, &path, generation),
        }
    }

    #[test]
    fn values_round_trip_in_memory() {
        let s = store(None, "ns", 1 << 20, 1 << 20);
        assert!(get(&s, "a", &[]).is_none());
        set(&s, "a", "one", &[]);
        assert_eq!(get(&s, "a", &[]).unwrap().value, "one");
        set(&s, "a", "two", &[]);
        assert_eq!(get(&s, "a", &[]).unwrap().value, "two");
        s.delete("a");
        assert!(get(&s, "a", &[]).is_none());
    }

    /// Tag times have millisecond resolution and only a revalidation
    /// strictly after a write affects it (as in Next's FileSystemCache).
    fn tick() {
        std::thread::sleep(std::time::Duration::from_millis(2));
    }

    #[test]
    fn tags_expire_or_mark_stale_like_next() {
        let s = store(None, "ns", 1 << 20, 1 << 20);
        set(&s, "a", "v", &["t1"]);
        set(&s, "b", "v", &[]);
        tick();
        // Immediate expiration (no durations).
        s.revalidate(&["t1".into()], None, false);
        assert!(get(&s, "a", &[]).is_none());
        // Tags given at lookup (softTags) count too.
        assert!(get(&s, "b", &["t1"]).is_none());
        // Entries written after the revalidation are fresh.
        tick();
        set(&s, "a", "v2", &["t1"]);
        assert_eq!(get(&s, "a", &[]).unwrap().value, "v2");
        tick();
        // Stale-while-revalidate: stale now, expired later.
        s.revalidate(&["t1".into()], Some(60.0), true);
        let hit = get(&s, "a", &[]).unwrap();
        assert!(hit.stale);
        // Durations without expire: stale only.
        set(&s, "c", "v", &["t2"]);
        tick();
        s.revalidate(&["t2".into()], None, true);
        assert!(get(&s, "c", &[]).unwrap().stale);
    }

    #[test]
    fn memory_limit_evicts_least_recently_used() {
        let s = store(None, "ns", 10, 1 << 20);
        set(&s, "a", "aaaa", &[]);
        set(&s, "b", "bbbb", &[]);
        assert!(get(&s, "a", &[]).is_some()); // a is now most recent
        set(&s, "c", "cccc", &[]); // 12 bytes > 10: b goes
        assert!(get(&s, "b", &[]).is_none());
        assert!(get(&s, "a", &[]).is_some());
        assert!(get(&s, "c", &[]).is_some());
        assert!(s.entries().1 <= 10);
    }

    #[test]
    fn oversized_keys_and_tag_sets_are_not_stored() {
        let s = store(None, "ns", 1 << 20, 1 << 20);
        let long_key = "k".repeat(MAX_KEY_BYTES + 1);
        set(&s, &long_key, "v", &[]);
        assert!(get(&s, &long_key, &[]).is_none());
        let many: Vec<String> = (0..=MAX_ENTRY_TAGS).map(|i| format!("t{i}")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        set(&s, "a", "v", &many);
        assert!(get(&s, "a", &[]).is_none());
        set(&s, "b", "v", &[&"t".repeat(MAX_TAG_BYTES + 1)]);
        assert!(get(&s, "b", &[]).is_none());
        assert_eq!(s.entries().0, 0);
    }

    #[test]
    fn oversized_values_are_not_stored() {
        let s = store(None, "ns", 1 << 20, 1 << 20);
        set(&s, "a", "small", &[]);
        let big = "x".repeat(2 * 1024 * 1024);
        set(&s, "a", &big, &[]);
        // The old value is dropped too: it is outdated.
        assert!(get(&s, "a", &[]).is_none());
    }

    #[test]
    fn entries_persist_and_tags_survive_restart() {
        let dir = temp_dir("persist");
        {
            let s = store(Some(dir.clone()), "v1-ns", 1 << 20, 1 << 20);
            set(&s, "a", "persisted", &["t"]);
            set(&s, "b", "doomed", &["gone"]);
            tick();
            if let Some(tags) = s.revalidate(&["gone".into()], None, false) {
                s.persist_tags(tags);
            }
        }
        let s = store(Some(dir.clone()), "v1-ns", 1 << 20, 1 << 20);
        assert_eq!(get(&s, "a", &[]).unwrap().value, "persisted");
        assert!(get(&s, "b", &[]).is_none(), "revalidated before restart");
        // Files are private to this user.
        use std::os::unix::fs::PermissionsExt;
        let file = dir.join("v1-ns/entries").join(file_name("a"));
        assert_eq!(
            fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            fs::metadata(dir.join("v1-ns"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn other_namespaces_are_never_read_and_are_removed() {
        let dir = temp_dir("ns");
        {
            let s = store(Some(dir.clone()), "v1-build-a", 1 << 20, 1 << 20);
            set(&s, "a", "from build a", &[]);
        }
        let s = store(Some(dir.clone()), "v1-build-b", 1 << 20, 1 << 20);
        assert!(get(&s, "a", &[]).is_none());
        assert!(!dir.join("v1-build-a").exists());
        // Unrelated directories in the cache root are left alone.
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupted_and_foreign_files_are_misses_and_removed() {
        let dir = temp_dir("corrupt");
        {
            let s = store(Some(dir.clone()), "v1-ns", 1 << 20, 1 << 20);
            set(&s, "a", "original value", &[]);
            set(&s, "b", "other", &[]);
        }
        let entries = dir.join("v1-ns/entries");
        // Flip a byte in a's value: checksum mismatch on read.
        let path_a = entries.join(file_name("a"));
        let mut bytes = fs::read(&path_a).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        fs::write(&path_a, bytes).unwrap();
        // Truncate b to its first bytes: rejected at startup.
        let path_b = entries.join(file_name("b"));
        fs::write(&path_b, b"NXRC").unwrap();
        // A file whose name does not match its key's hash.
        fs::write(entries.join("0000"), b"junk").unwrap();
        // A torn temporary file.
        fs::write(dir.join("v1-ns/tmp/x.tmp"), b"partial").unwrap();

        let s = store(Some(dir.clone()), "v1-ns", 0, 1 << 20);
        assert!(get(&s, "a", &[]).is_none());
        assert!(get(&s, "b", &[]).is_none());
        assert!(!path_a.exists() && !path_b.exists());
        assert!(!entries.join("0000").exists());
        assert_eq!(fs::read_dir(dir.join("v1-ns/tmp")).unwrap().count(), 0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn disk_limit_removes_least_recently_used_files() {
        let dir = temp_dir("disk");
        let s = store(Some(dir.clone()), "v1-ns", 1 << 20, 600);
        for key in ["a", "b", "c", "d"] {
            set(&s, key, &"x".repeat(200), &[]);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let (_, _, disk) = s.entries();
        assert!(disk <= 600, "{disk}");
        assert!(get(&s, "a", &[]).is_none(), "oldest evicted");
        assert!(get(&s, "d", &[]).is_some());
        let files = fs::read_dir(dir.join("v1-ns/entries")).unwrap().count() as u64;
        assert_eq!(files, s.entries().0);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn values_evicted_from_memory_are_reloaded_from_disk() {
        let dir = temp_dir("reload");
        let s = store(Some(dir.clone()), "v1-ns", 300, 1 << 20);
        set(&s, "a", &"a".repeat(200), &[]);
        set(&s, "b", &"b".repeat(200), &[]);
        // a's value left memory but its file remains.
        assert_eq!(get(&s, "a", &[]).unwrap().value, "a".repeat(200));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_symlinked_cache_directory_is_refused() {
        let dir = temp_dir("symlink");
        fs::create_dir_all(&dir).unwrap();
        let target = temp_dir("symlink-target");
        fs::create_dir_all(&target).unwrap();
        std::os::unix::fs::symlink(&target, dir.join("v1-ns")).unwrap();
        let result = CacheStore::open(
            CacheConfig {
                dir: Some(dir.clone()),
                ..CacheConfig::default()
            },
            "v1-ns",
            Arc::new(Counters::default()),
        );
        assert!(result.is_err());
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&target);
    }

    #[test]
    fn concurrent_writers_leave_the_newest_value_on_disk() {
        let dir = temp_dir("concurrent");
        let s = Arc::new(store(Some(dir.clone()), "v1-ns", 1 << 20, 1 << 22));
        let threads: Vec<_> = (0..8)
            .map(|t| {
                let s = s.clone();
                std::thread::spawn(move || {
                    for i in 0..50 {
                        let write = s.set("k", Bytes::from(format!("{t}-{i}")), vec![]);
                        if let Some(write) = write {
                            s.persist(write);
                        }
                    }
                })
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let in_memory = get(&s, "k", &[]).unwrap().value;
        drop(s);
        let reopened = store(Some(dir.clone()), "v1-ns", 1 << 20, 1 << 22);
        assert_eq!(get(&reopened, "k", &[]).unwrap().value, in_memory);
        let _ = fs::remove_dir_all(&dir);
    }
}
