//! The cache shared by every isolate: Next.js `fetch` caching,
//! `unstable_cache`, and the response cache (ISR: `APP_PAGE` and
//! `APP_ROUTE` entries), stored in Rust and optionally persisted to disk.
//!
//! JavaScript reaches it through a `CacheHandler` (Next's documented
//! interface, `js/03_server.js`) and a few ops; it passes cache keys, tags
//! and opaque values, never paths. Entries live in memory up to a byte budget
//! and, with a directory, in files named by the SHA-256 of their key under a
//! namespace derived from the build ID and the JavaScript environment, so a
//! different build or environment never reads another's entries.
//!
//! Tags are recorded as Next's `FileSystemCache` records them (a manifest of
//! `stale`/`expired` times per tag), once for the whole pool. The store does
//! not judge entries by them: every isolate mirrors the manifest into Next's
//! own `tagsManifest` module (`tag_sync`, versioned) and Next's
//! `areTagsExpired`/`areTagsStale` decide, as under `next start`.
//!
//! Files are written to a temporary name and renamed into place, carry a
//! checksum, and are verified when read: a torn or corrupted file is a miss
//! and is removed. Disk access runs on blocking threads, never on a V8
//! thread.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
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
/// Tag changes kept for incremental `tag_sync`; an isolate further behind
/// receives the whole manifest.
const TAG_LOG: usize = 4096;
/// Response entries evicted from memory without a disk copy are remembered
/// (key and time only) so that a later read misses instead of falling back
/// to the older build-time prerender; at most this many.
const MAX_EVICTED_MARKERS: usize = 100_000;
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
    /// Larger data-cache values (`fetch`, `unstable_cache`) are not stored
    /// (Next's own limit is 2 MB).
    pub max_entry_bytes: u64,
    /// Larger response-cache values (ISR pages and route handlers) are not
    /// stored; Next sets no limit of its own for them.
    pub max_response_bytes: u64,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            dir: None,
            max_memory_bytes: 50 * 1024 * 1024,
            max_disk_bytes: 1024 * 1024 * 1024,
            max_entry_bytes: 2 * 1024 * 1024,
            max_response_bytes: 16 * 1024 * 1024,
        }
    }
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// What an entry holds: Next's data cache (`FETCH`) or its response cache
/// (`APP_PAGE`, `APP_ROUTE`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum EntryKind {
    #[default]
    Fetch,
    Response,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
struct TagState {
    /// Entries written before this are stale.
    stale: Option<u64>,
    /// Entries written before this are gone, once this time has passed.
    expired: Option<u64>,
}

struct Entry {
    /// `None` when evicted from memory; then the entry is on disk, or, for
    /// a response entry, only remembered as evicted (`size` 0).
    value: Option<Bytes>,
    kind: EntryKind,
    size: u64,
    last_modified: u64,
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
    /// Bumped by every tag change; `tag_log` holds the changes after
    /// `tag_log_base` (version, tag).
    tags_version: u64,
    tag_log: VecDeque<(u64, String)>,
    tag_log_base: u64,
    evicted_markers: usize,
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
        if entry.value.is_none() && !entry.on_disk {
            self.evicted_markers -= 1;
        }
        Some(entry)
    }

    fn record_tag(&mut self, tag: &str) {
        self.tags_version += 1;
        let version = self.tags_version;
        self.tag_log.push_back((version, tag.to_owned()));
        while self.tag_log.len() > TAG_LOG {
            if let Some((dropped, _)) = self.tag_log.pop_front() {
                self.tag_log_base = dropped;
            }
        }
    }
}

/// A cache hit. Tags are not judged here (see the module comment).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Hit {
    /// `None` for a response entry that left memory without a disk copy:
    /// a newer value existed, so the caller must not fall back to the
    /// build-time prerender.
    pub value: Option<String>,
    /// Milliseconds since the epoch.
    pub last_modified: f64,
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

/// Tag changes for one isolate's mirror of the manifest (`tag_sync`).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TagSync {
    pub version: f64,
    /// The mirror must be replaced, not updated.
    pub full: bool,
    /// `[tag, stale, expired]`; both `None` means the tag was forgotten.
    pub tags: Vec<(String, Option<f64>, Option<f64>)>,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SetOptions {
    pub last_modified: Option<u64>,
    pub if_absent: bool,
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
    leases: Mutex<HashMap<String, Lease>>,
    next_lease: std::sync::atomic::AtomicU64,
}

/// One isolate regenerating a response entry that others found missing
/// (single flight across the pool; Next's own batcher covers one isolate).
struct Lease {
    id: u64,
    until: std::time::Instant,
    released: Arc<tokio::sync::Notify>,
}

/// What `acquire_lease` did.
pub(crate) enum LeaseOutcome {
    /// The caller regenerates and releases the lease with this id.
    Acquired(u64),
    /// Another isolate was regenerating; it finished (or `timed_out`).
    Waited { timed_out: bool },
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileHeader {
    key: String,
    last_modified: u64,
    tags: Vec<String>,
    sha256: String,
    /// Absent in files written before response entries existed.
    #[serde(default)]
    kind: EntryKind,
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

fn encode(
    key: &str,
    value: &[u8],
    last_modified: u64,
    tags: &[String],
    kind: EntryKind,
) -> Vec<u8> {
    let header = serde_json::to_vec(&FileHeader {
        key: key.to_owned(),
        last_modified,
        tags: tags.to_vec(),
        sha256: hex(&Sha256::digest(value)),
        kind,
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
                let mut found: Vec<(u64, String, u64, EntryKind)> = Vec::new();
                for file in fs::read_dir(&disk.entries).map_err(fail)?.flatten() {
                    let path = file.path();
                    match decode(&path, false) {
                        Ok((header, None))
                            if path
                                .file_name()
                                .is_some_and(|n| n.to_string_lossy() == file_name(&header.key)) =>
                        {
                            let size = file.metadata().map_or(0, |m| m.len());
                            found.push((header.last_modified, header.key, size, header.kind));
                        }
                        _ => {
                            metrics::add(&counters.cache_corrupt, 1);
                            let _ = fs::remove_file(&path);
                        }
                    }
                }
                // Oldest first, so recency follows modification time.
                found.sort_by_key(|(modified, ..)| *modified);
                for (last_modified, key, size, kind) in found {
                    let stamp = state.next_stamp;
                    state.next_stamp += 1;
                    state.lru.insert(stamp, key.clone());
                    state.disk_bytes += size;
                    state.entries.insert(
                        key,
                        Entry {
                            value: None,
                            kind,
                            size,
                            last_modified,
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
        // Isolates start at version 0, below the base: their first sync
        // receives the whole manifest.
        state.tags_version = 1;
        state.tag_log_base = 1;
        let store = Self {
            state: Mutex::new(state),
            disk,
            config,
            counters,
            write_lock: Mutex::new(()),
            leases: Mutex::new(HashMap::new()),
            next_lease: std::sync::atomic::AtomicU64::new(1),
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

    /// Looks a key up in memory.
    pub fn lookup(&self, key: &str) -> Lookup {
        if key.len() > MAX_KEY_BYTES {
            metrics::add(&self.counters.cache_misses, 1);
            return Lookup::Miss;
        }
        let mut state = self.lock();
        let Some(entry) = state.entries.get(key) else {
            drop(state);
            metrics::add(&self.counters.cache_misses, 1);
            return Lookup::Miss;
        };
        let last_modified = entry.last_modified as f64;
        match (&entry.value, entry.on_disk) {
            (Some(value), _) => {
                let hit = Hit {
                    value: Some(String::from_utf8_lossy(value).into_owned()),
                    last_modified,
                };
                state.touch(key);
                drop(state);
                metrics::add(&self.counters.cache_hits, 1);
                Lookup::Hit(hit)
            }
            (None, true) => {
                let generation = entry.generation;
                let path = self
                    .disk
                    .as_ref()
                    .expect("entries are on disk only with a directory")
                    .entries
                    .join(file_name(key));
                Lookup::OnDisk { path, generation }
            }
            (None, false) => {
                // An evicted response entry: a miss that must not fall back
                // to the build's prerender.
                drop(state);
                metrics::add(&self.counters.cache_misses, 1);
                Lookup::Hit(Hit {
                    value: None,
                    last_modified,
                })
            }
        }
    }

    /// Reads an entry that was only on disk (blocking) and keeps its value
    /// in memory.
    pub fn load(&self, key: &str, path: &Path, generation: u64) -> Option<Hit> {
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
            return match self.lookup(key) {
                Lookup::Hit(hit) => Some(hit),
                _ => None,
            };
        }
        let value = Bytes::from(value);
        let entry = state.entries.get_mut(key).expect("checked above");
        let last_modified = entry.last_modified;
        if entry.value.is_none() {
            entry.value = Some(value.clone());
            let size = entry.size;
            state.memory_bytes += size;
        }
        state.touch(key);
        let removed = self.enforce_limits(&mut state);
        drop(state);
        self.delete_files(removed);
        metrics::add(&self.counters.cache_hits, 1);
        Some(Hit {
            value: Some(String::from_utf8_lossy(&value).into_owned()),
            last_modified: last_modified as f64,
        })
    }

    /// Stores a value: in memory, visible to every isolate before this
    /// returns. Returns the file write to perform, if the cache persists.
    #[cfg(test)]
    pub fn set(
        &self,
        key: &str,
        value: Bytes,
        tags: Vec<String>,
        kind: EntryKind,
    ) -> Option<PendingWrite> {
        self.set_with(key, value, tags, kind, SetOptions::default())
    }

    /// `set`, with the time the value was produced (a build-time prerender
    /// keeps its own) and, with `if_absent`, only when the store holds
    /// nothing for the key (an older seed never replaces a newer value).
    pub fn set_with(
        &self,
        key: &str,
        value: Bytes,
        tags: Vec<String>,
        kind: EntryKind,
        options: SetOptions,
    ) -> Option<PendingWrite> {
        let size = value.len() as u64;
        let limit = match kind {
            EntryKind::Fetch => self.config.max_entry_bytes,
            EntryKind::Response => self.config.max_response_bytes,
        };
        let oversized_tags =
            tags.len() > MAX_ENTRY_TAGS || tags.iter().any(|t| t.len() > MAX_TAG_BYTES);
        if size > limit || key.len() > MAX_KEY_BYTES || oversized_tags {
            metrics::add(&self.counters.cache_too_large, 1);
            // Whatever was there is outdated now.
            self.delete(key);
            return None;
        }
        let last_modified = options.last_modified.unwrap_or_else(now_ms);
        let mut state = self.lock();
        if options.if_absent && state.entries.contains_key(key) {
            return None;
        }
        // An older file stays until the new one is renamed over it, so a
        // crash in between leaves the older complete entry, never a torn one.
        state.remove(key);
        let generation = state.next_generation;
        state.next_generation += 1;
        let stamp = state.next_stamp;
        state.next_stamp += 1;
        let write = self.disk.as_ref().map(|disk| PendingWrite {
            key: key.to_owned(),
            generation,
            path: disk.entries.join(file_name(key)),
            contents: encode(key, &value, last_modified, &tags, kind),
        });
        state.memory_bytes += size;
        state.lru.insert(stamp, key.to_owned());
        state.entries.insert(
            key.to_owned(),
            Entry {
                value: Some(value),
                kind,
                size,
                last_modified,
                generation,
                on_disk: false,
                lru: stamp,
            },
        );
        metrics::add(&self.counters.cache_sets, 1);
        if kind == EntryKind::Response {
            metrics::add(&self.counters.cache_response_sets, 1);
        }
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
            // An older file for this key would come back after a restart
            // although memory holds a newer value: remove it.
            let _ = fs::remove_file(&write.path);
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
            Some(_) => {
                // Replaced while writing: the newer write renames over it.
            }
            None => {
                // Removed while writing.
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
            state.record_tag(tag);
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
                state.record_tag(&tag);
            }
        }
        metrics::add(&self.counters.cache_revalidations, tags.len() as u64);
        self.disk
            .as_ref()
            .map(|_| serde_json::to_vec(&state.tags).expect("tags serialize"))
    }

    /// The tag changes after `since` (a version an isolate synced to
    /// before; 0 for none), or the whole manifest if the log no longer
    /// reaches back that far.
    pub fn tag_sync(&self, since: u64) -> TagSync {
        let state = self.lock();
        let version = state.tags_version;
        let entry = |tag: &str| {
            let s = state.tags.get(tag).copied().unwrap_or_default();
            (
                tag.to_owned(),
                s.stale.map(|v| v as f64),
                s.expired.map(|v| v as f64),
            )
        };
        if since == version {
            return TagSync {
                version: version as f64,
                full: false,
                tags: Vec::new(),
            };
        }
        if since < state.tag_log_base || since > version {
            return TagSync {
                version: version as f64,
                full: true,
                tags: state.tags.keys().map(|tag| entry(tag)).collect(),
            };
        }
        let mut seen = HashSet::new();
        let tags = state
            .tag_log
            .iter()
            .filter(|(v, _)| *v > since)
            .filter(|(_, tag)| seen.insert(tag.as_str()))
            .map(|(_, tag)| entry(tag))
            .collect();
        TagSync {
            version: version as f64,
            full: false,
            tags,
        }
    }

    /// Whether any of `tags` was revalidated (marked stale or expired)
    /// after `since_ms` (Unix milliseconds).
    pub fn revalidated_since(&self, tags: &[String], since_ms: u64) -> bool {
        let now = now_ms();
        let state = self.lock();
        tags.iter().any(|tag| {
            state.tags.get(tag).is_some_and(|s| {
                // A profiled revalidation marks the tag stale now and sets
                // its expiry in the future; only a reached expiry counts.
                s.stale.is_some_and(|at| at > since_ms)
                    || s.expired.is_some_and(|at| at > since_ms && at <= now)
            })
        })
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

    /// Takes the regeneration lease for `key`, or waits (at most `ttl`) for
    /// the isolate holding it. A lease lasts `ttl` at most, so a holder
    /// that never releases it delays others by that much, once. No lock
    /// is held while waiting.
    pub async fn acquire_lease(&self, key: &str, ttl: std::time::Duration) -> LeaseOutcome {
        let now = std::time::Instant::now();
        let (released, until) = {
            let mut leases = self.leases.lock().unwrap_or_else(|e| e.into_inner());
            match leases.get(key) {
                Some(lease) if lease.until > now => (lease.released.clone(), lease.until),
                _ => {
                    let id = self
                        .next_lease
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    leases.insert(
                        key.to_owned(),
                        Lease {
                            id,
                            until: now + ttl,
                            released: Arc::new(tokio::sync::Notify::new()),
                        },
                    );
                    if leases.len() > 4096 {
                        leases.retain(|_, lease| lease.until > now);
                    }
                    return LeaseOutcome::Acquired(id);
                }
            }
        };
        // notify_waiters wakes every future created before it; a release
        // between dropping the lock and creating this one is caught by the
        // re-check.
        let notified = released.notified();
        let gone = self
            .leases
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(key)
            .is_none_or(|lease| !Arc::ptr_eq(&lease.released, &released));
        if gone {
            metrics::add(&self.counters.cache_regen_waits, 1);
            return LeaseOutcome::Waited { timed_out: false };
        }
        let timed_out = tokio::time::timeout_at(until.into(), notified)
            .await
            .is_err();
        metrics::add(&self.counters.cache_regen_waits, 1);
        if timed_out {
            metrics::add(&self.counters.cache_regen_wait_timeouts, 1);
        }
        LeaseOutcome::Waited { timed_out }
    }

    /// Ends a lease taken with `acquire_lease` (after storing the entry, or
    /// when the request that took it ends) and wakes the waiters.
    pub fn release_lease(&self, key: &str, id: u64) {
        let mut leases = self.leases.lock().unwrap_or_else(|e| e.into_inner());
        if leases.get(key).is_some_and(|lease| lease.id == id)
            && let Some(lease) = leases.remove(key)
        {
            lease.released.notify_waiters();
        }
    }

    /// Evicts values from memory and entries from disk beyond the limits.
    /// Returns keys whose files must be deleted.
    fn enforce_limits(&self, state: &mut State) -> Vec<String> {
        let mut delete = Vec::new();
        let mut stamps: Vec<u64> = Vec::new();
        if state.memory_bytes > self.config.max_memory_bytes
            || state.disk_bytes > self.config.max_disk_bytes
            || state.evicted_markers > MAX_EVICTED_MARKERS
        {
            stamps.extend(state.lru.keys().copied());
        }
        for stamp in stamps {
            let over_memory = state.memory_bytes > self.config.max_memory_bytes;
            let over_disk = state.disk_bytes > self.config.max_disk_bytes;
            let over_markers = state.evicted_markers > MAX_EVICTED_MARKERS;
            if !over_memory && !over_disk && !over_markers {
                break;
            }
            let Some(key) = state.lru.get(&stamp).cloned() else {
                continue;
            };
            let entry = state.entries.get_mut(&key).expect("lru and entries agree");
            let marker = entry.value.is_none() && !entry.on_disk;
            if marker {
                if over_markers {
                    state.remove(&key);
                }
            } else if over_disk && entry.on_disk {
                // Gone entirely.
                let entry = state.remove(&key).expect("present");
                metrics::add(&self.counters.cache_evictions, 1);
                if entry.on_disk {
                    delete.push(key);
                }
            } else if over_memory && !entry.on_disk {
                metrics::add(&self.counters.cache_evictions, 1);
                if entry.kind == EntryKind::Response {
                    // Keep the key and time: a later read misses rather
                    // than serving the older build-time prerender.
                    let size = entry.size;
                    entry.value = None;
                    entry.size = 0;
                    state.memory_bytes -= size;
                    state.evicted_markers += 1;
                } else {
                    state.remove(&key);
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
                max_response_bytes: 1024 * 1024,
            },
            ns,
            Arc::new(Counters::default()),
        )
        .unwrap()
    }

    fn set_kind(store: &CacheStore, key: &str, value: &str, tags: &[&str], kind: EntryKind) {
        let write = store.set(
            key,
            Bytes::from(value.to_owned()),
            tags.iter().map(|t| (*t).to_owned()).collect(),
            kind,
        );
        if let Some(write) = write {
            store.persist(write);
        }
    }

    fn set(store: &CacheStore, key: &str, value: &str, tags: &[&str]) {
        set_kind(store, key, value, tags, EntryKind::Fetch);
    }

    fn hit(store: &CacheStore, key: &str) -> Option<Hit> {
        match store.lookup(key) {
            Lookup::Hit(hit) => Some(hit),
            Lookup::Miss => None,
            Lookup::OnDisk { path, generation } => store.load(key, &path, generation),
        }
    }

    /// The value, if the store holds one.
    fn get(store: &CacheStore, key: &str, _tags: &[&str]) -> Option<String> {
        hit(store, key).and_then(|hit| hit.value)
    }

    fn tag_map(sync: &TagSync) -> HashMap<String, (Option<f64>, Option<f64>)> {
        sync.tags
            .iter()
            .map(|(tag, stale, expired)| (tag.clone(), (*stale, *expired)))
            .collect()
    }

    #[test]
    fn values_round_trip_in_memory() {
        let s = store(None, "ns", 1 << 20, 1 << 20);
        assert!(get(&s, "a", &[]).is_none());
        set(&s, "a", "one", &[]);
        assert_eq!(get(&s, "a", &[]).unwrap(), "one");
        set(&s, "a", "two", &[]);
        assert_eq!(get(&s, "a", &[]).unwrap(), "two");
        s.delete("a");
        assert!(get(&s, "a", &[]).is_none());
    }

    /// Tag times have millisecond resolution and only a revalidation
    /// strictly after a write affects it (as in Next's FileSystemCache).
    fn tick() {
        std::thread::sleep(std::time::Duration::from_millis(2));
    }

    /// Tag times are recorded as FileSystemCache.revalidateTag records
    /// them; judging is left to Next (the store returns entries as stored).
    #[test]
    fn tags_are_recorded_like_next_and_synced_by_version() {
        let s = store(None, "ns", 1 << 20, 1 << 20);
        set(&s, "a", "v", &["t1"]);
        let first = s.tag_sync(0);
        assert!(first.full && first.tags.is_empty(), "{first:?}");
        let v0 = first.version as u64;
        tick();
        let before = now_ms() as f64;
        // Immediate expiration (no durations).
        s.revalidate(&["t1".into()], None, false);
        // The store does not judge: the entry is still returned.
        assert_eq!(get(&s, "a", &[]).as_deref(), Some("v"));
        let sync = s.tag_sync(v0);
        assert!(!sync.full);
        let (stale, expired) = tag_map(&sync)["t1"];
        assert_eq!(stale, None);
        assert!(expired.unwrap() >= before);
        // Stale-while-revalidate keeps the stale mark and moves the expiry.
        tick();
        s.revalidate(&["t1".into(), "t2".into()], Some(60.0), true);
        let sync2 = s.tag_sync(sync.version as u64);
        let tags = tag_map(&sync2);
        assert_eq!(tags.len(), 2);
        let (stale, expired) = tags["t1"];
        assert!(stale.unwrap() > before);
        assert!(expired.unwrap() >= stale.unwrap() + 59_000.0);
        // Durations without expire: stale only, an earlier expiry is kept.
        tick();
        s.revalidate(&["t1".into()], None, true);
        let (_, kept) = tag_map(&s.tag_sync(sync2.version as u64))["t1"];
        assert_eq!(kept, expired);
        // Up to date: nothing to send.
        let current = s.tag_sync(0).version as u64;
        assert!(s.tag_sync(current).tags.is_empty());
    }

    #[test]
    fn an_isolate_behind_the_tag_log_gets_the_whole_manifest() {
        let s = store(None, "ns", 1 << 20, 1 << 20);
        let start = s.tag_sync(0).version as u64;
        for i in 0..(TAG_LOG + 10) {
            s.revalidate(&[format!("t{i}")], None, false);
        }
        let behind = s.tag_sync(start);
        assert!(behind.full);
        assert_eq!(behind.tags.len(), TAG_LOG + 10);
        // Close behind: only the changes.
        let recent = s.tag_sync(behind.version as u64 - 3);
        assert!(!recent.full);
        assert_eq!(recent.tags.len(), 3);
    }

    #[test]
    fn evicted_response_entries_are_remembered_not_forgotten() {
        let s = store(None, "ns", 10, 1 << 20);
        set_kind(&s, "page", "aaaa", &[], EntryKind::Response);
        set(&s, "fetch", "bbbb", &[]);
        set(&s, "other", "cccccccc", &[]);
        // Over 10 bytes: the oldest go. The page leaves a marker (a hit
        // without a value); the fetch entry is simply gone.
        let page = hit(&s, "page").expect("marker");
        assert!(page.value.is_none());
        assert!(hit(&s, "fetch").is_none());
        assert_eq!(get(&s, "other", &[]).as_deref(), Some("cccccccc"));
        // A new value replaces the marker.
        s.delete("other");
        set_kind(&s, "page", "new", &[], EntryKind::Response);
        assert_eq!(get(&s, "page", &[]).as_deref(), Some("new"));
    }

    #[test]
    fn response_entries_have_their_own_size_limit() {
        let s = CacheStore::open(
            CacheConfig {
                dir: None,
                max_memory_bytes: 1 << 20,
                max_disk_bytes: 1 << 20,
                max_entry_bytes: 4,
                max_response_bytes: 64,
            },
            "ns",
            Arc::new(Counters::default()),
        )
        .unwrap();
        set(&s, "f", "too long", &[]);
        assert!(get(&s, "f", &[]).is_none());
        set_kind(&s, "p", "page value", &[], EntryKind::Response);
        assert_eq!(get(&s, "p", &[]).as_deref(), Some("page value"));
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
        assert_eq!(get(&s, "a", &[]).unwrap(), "persisted");
        // The tag state survives; judging it is Next's (see tag_sync).
        assert!(s.revalidated_since(&["gone".to_owned()], 1));
        let sync = s.tag_sync(0);
        assert!(sync.full && tag_map(&sync)["gone"].1.is_some());
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
        assert_eq!(get(&s, "a", &[]).unwrap(), "a".repeat(200));
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
                        let write = s.set(
                            "k",
                            Bytes::from(format!("{t}-{i}")),
                            vec![],
                            EntryKind::Fetch,
                        );
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
        let in_memory = get(&s, "k", &[]).unwrap();
        drop(s);
        let reopened = store(Some(dir.clone()), "v1-ns", 1 << 20, 1 << 22);
        assert_eq!(get(&reopened, "k", &[]).unwrap(), in_memory);
        let _ = fs::remove_dir_all(&dir);
    }

    /// What the server checks before serving a prerendered page: a
    /// revalidation of one of its tags after the prerender was written.
    #[test]
    fn revalidated_since_sees_tag_events_after_a_time() {
        let s = store(None, "revalidated-since", 1 << 20, 0);
        let tags = vec!["_N_T_/page".to_owned(), "_N_T_/layout".to_owned()];
        let written = now_ms();
        assert!(!s.revalidated_since(&tags, written));
        std::thread::sleep(std::time::Duration::from_millis(2));
        s.revalidate(&["_N_T_/other".to_owned()], None, false);
        assert!(!s.revalidated_since(&tags, written), "another path");
        s.revalidate(&["_N_T_/layout".to_owned()], Some(3600.0), true);
        assert!(s.revalidated_since(&tags, written), "stale (profiled)");
        assert!(
            !s.revalidated_since(&tags, now_ms() + 1_000),
            "before a later write"
        );
        let s = store(None, "revalidated-since-2", 1 << 20, 0);
        std::thread::sleep(std::time::Duration::from_millis(2));
        s.revalidate(&["_N_T_/page".to_owned()], None, false);
        assert!(s.revalidated_since(&tags, written), "expired");
    }
}
