# RE-5D: cache serialization and allocation

The shared data and ISR cache (`crates/nextrs-runtime/src/cache.rs`, one
store for all isolates) and the compression cache
(`crates/nextrs-server/src/compress_cache.rs`), audited for copies,
conversions, duplicate representations and lock hold time. One change
(§3). The RE-4.1 cross-isolate clock fix (`performance.timeOrigin`,
`3973254`) is untouched, and so are timestamps and invalidation.

## 1. How a value moves

| Step | Representation | Copies |
|---|---|---|
| `set` from JavaScript (`op_nextrs_cache_set`, `#[string] Option<String>`) | V8 string → Rust `String` → `Bytes` (no copy: `Bytes::from(String)`) | 1 (transcoding, unavoidable) |
| In memory | one `Bytes` per entry, shared by every isolate | 0 |
| Persisted (`--cache-dir`) | file = header (key, last-modified, tags, SHA-256) + value; temporary name, renamed | before: the value **copied into a file image** under the store lock; now: written from the entry's own bytes (§3) |
| `get` hit (`op_nextrs_cache_get`) | `Bytes` → `String` (`from_utf8_lossy(..).into_owned()`) → serde_v8 → V8 string | 2 |
| `get` when the isolate's decoded copy is current (`unchanged`) | nothing (generation check) | 0 |
| From disk | `Vec` read on a blocking thread, verified, then as a hit | 1 + hit |

Next parses each value in JavaScript (JSON) into its own objects. The
isolate keeps that decoded copy for reuse, up to 16 MiB per isolate, and
asks with its generation. So the two hit copies happen once per isolate
per entry generation, not per request. In the Phase 3A `isr-hit`
workload, the steady state is `unchanged` lookups.

## 2. Findings

| Finding | Effect | Decision |
|---|---|---|
| With a cache directory, `set_with` built the file image (`encode`: SHA-256 of the value, a JSON header, and a **copy of the whole value**) **while holding the store's lock**, which every isolate's lookups take | each ISR or data-cache write held all isolates' cache access for a hash and a copy of the value (SHA-256 at about 0.5–1 GB/s: about 0.1–0.2 ms for a 100 KB page), and kept a second copy of the value until it was persisted | **changed** (§3) |
| Hit: `from_utf8_lossy(..).into_owned()` copies the value into a Rust `String` before serde_v8 makes the V8 string | one value-sized transient per first lookup per isolate per generation | not changed: no measurable steady-state benefit (reuse already avoids it). A `Serialize` newtype over `&str` borrowed from the `Bytes` would remove it, and is safe since values are UTF-8 strings (the lossy fallback keeps behaviour for a corrupted file) |
| Tags manifest persisted as JSON on each revalidation (`persist_tags`) | one serialization per revalidation, off the V8 threads | unchanged |
| Eviction: least recently used by a `BTreeMap` stamp, under the lock | O(log n) per eviction | unchanged; no contention measured |
| Response entries evicted without a disk copy leave a marker (key and time, at most 100 000) | bounded | unchanged (correctness: a later read misses instead of serving the older build-time prerender) |
| Compression cache: one gzip copy per immutable body, keyed by `Cached::id` or the file's canonical path; fills coalesced (`OnceCell`) | no duplicate fills | unchanged; RE-5B keeps ids stable across reloads |

## 3. The change

`set_with`:

- **Before the lock:** it computes the file header (`encode_header`:
  magic, version, the header length, the JSON header with the value's
  SHA-256).
- **Under the lock:** it does only what the lock guards: generation,
  stamp, entry and limits.
- **Persisting:** the pending write keeps the header and the entry's own
  `Bytes` (a reference, not a copy). `persist` writes them one after the
  other to the temporary file, which is then renamed into place, as
  before.

| | Before | After |
|---|---|---|
| Work under the store lock per persisted set | SHA-256 of the value + JSON header + value copy | none of these |
| Memory per persisted set until written | entry value + a file image (header + value) | entry value + header |
| File bytes | magic, version, length, header, value | identical |

Unchanged:

- `last_modified`, still read once, before the entry is visible;
- generation order, which is still decided under the lock;
- crash safety: temporary file, flush, rename, and an older complete
  file kept until then;
- checksum verification on read;
- disk accounting (header + value).

Without a cache directory, nothing changes: no header is made.

## 4. Tests

- `tests/cache.rs` (whole binary: persistence, restart, corruption,
  concurrent writers, tags, revalidation, eviction, the cross-isolate
  visibility tests) and the cache unit tests: see
  [RE-5-CORRECTNESS-VALIDATION](RE-5-CORRECTNESS-VALIDATION.md).
- The file format is unchanged; the restart tests read files written by
  the new path.

## 5. Measurement

No benchmark was run for this change alone.

- The lock-hold reduction applies only with `--cache-dir` and under
  concurrent writes. Neither the Phase 3A workloads nor the container
  campaign isolate it.
- Its effect is per write: a hash and a value-sized copy no longer under
  the lock, one value-sized copy fewer.
- Claimed as a structural improvement, not as a measured CPU reduction.
