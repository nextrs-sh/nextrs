//! The shared data cache seen from JavaScript, through the cache handler
//! NextRS offers to Next.js: visibility across isolates, isolate and
//! process restarts, pool size and environment changes, corrupted files,
//! disk failures, and JavaScript's lack of file access to it.
mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use common::*;
use nextrs_runtime::{CacheConfig, Executor, Limits};
use serde_json::Value;

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nextrs-cache-test-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn start(isolates: usize, dir: Option<&Path>, env: &[(&str, &str)]) -> Arc<Executor> {
    let mut config = config(Limits {
        isolates,
        ..small_limits()
    });
    config.cache = Some(CacheConfig {
        dir: dir.map(Path::to_path_buf),
        ..CacheConfig::default()
    });
    for (name, value) in env {
        config.env.insert((*name).to_owned(), (*value).to_owned());
    }
    Executor::start(config).expect("executor starts")
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~,".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

async fn op(executor: &Executor, params: &[(&str, &str)]) -> Value {
    let mut request = request("cache.js", &[]);
    request.url = format!(
        "/cache?{}",
        params
            .iter()
            .map(|(k, v)| format!("{k}={}", encode(v)))
            .collect::<Vec<_>>()
            .join("&")
    );
    let out = collect(executor, request).await;
    assert_eq!(out.status, 200, "{}", out.text());
    serde_json::from_slice(&out.body).unwrap()
}

async fn set(executor: &Executor, key: &str, value: &str, tags: &str) {
    let r = op(
        executor,
        &[
            ("op", "set"),
            ("key", key),
            ("value", value),
            ("tags", tags),
        ],
    )
    .await;
    assert!(r.get("error").is_none(), "{r}");
}

async fn get(executor: &Executor, key: &str) -> Option<String> {
    get_tagged(executor, key, "").await
}

/// A read as Next makes it: with the fetch's tags, which are what a tag
/// revalidation is judged against (FileSystemCache semantics).
async fn get_tagged(executor: &Executor, key: &str, tags: &str) -> Option<String> {
    let r = op(executor, &[("op", "get"), ("key", key), ("tags", tags)]).await;
    assert!(r.get("error").is_none(), "{r}");
    r["entry"]["body"].as_str().map(str::to_owned)
}

fn entry_files(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for ns in std::fs::read_dir(dir).unwrap().flatten() {
        if let Ok(entries) = std::fs::read_dir(ns.path().join("entries")) {
            files.extend(entries.flatten().map(|e| e.path()));
        }
    }
    files
}

#[tokio::test(flavor = "multi_thread")]
async fn the_handler_is_offered_only_when_the_cache_is_enabled() {
    let executor = start(1, None, &[]);
    assert_eq!(op(&executor, &[("op", "handler")]).await["installed"], true);
    let executor = executor_without_cache();
    assert_eq!(
        op(&executor, &[("op", "handler")]).await["installed"],
        false
    );
}

fn executor_without_cache() -> Arc<Executor> {
    let mut config = config(small_limits());
    config.cache = None;
    Executor::start(config).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn every_isolate_sees_one_cache() {
    let executor = start(4, None, &[]);
    set(&executor, "shared", "value-1", "t").await;
    // Concurrent readers spread over the pool.
    let readers: Vec<_> = (0..64)
        .map(|_| {
            let executor = executor.clone();
            tokio::spawn(async move { op(&executor, &[("op", "get"), ("key", "shared")]).await })
        })
        .collect();
    let mut isolates = std::collections::HashSet::new();
    for reader in readers {
        let r = reader.await.unwrap();
        assert_eq!(r["entry"]["body"], "value-1", "{r}");
        isolates.insert(r["isolate"].as_str().unwrap().to_owned());
    }
    assert!(
        isolates.len() > 1,
        "readers should have reached several isolates"
    );
    // A revalidation through one isolate applies to all.
    op(&executor, &[("op", "revalidate"), ("tags", "t")]).await;
    tokio::time::sleep(Duration::from_millis(2)).await;
    for _ in 0..16 {
        assert_eq!(get_tagged(&executor, "shared", "t").await, None);
    }
    let metrics = executor.metrics().cache.unwrap();
    // The store reports what it found; the 16 entries it returned after
    // the revalidation were then judged expired by the handler (Next's
    // rules), so they count as store hits.
    assert!(metrics.hits >= 80, "{metrics:?}");
}

#[tokio::test(flavor = "multi_thread")]
async fn entries_outlive_an_isolate_restart() {
    let executor = start(1, None, &[]);
    set(&executor, "k", "before-restart", "").await;
    let before = whoami(&executor).await;
    let spin = {
        let mut r = request("faults.js", &[("x-mode", "spin"), ("x-delay", "3000")]);
        r.url = "/spin".into();
        r
    };
    let _ = collect(&executor, spin).await;
    assert!(
        eventually(Duration::from_secs(5), async || whoami(&executor).await
            != before)
        .await,
        "the isolate should have been replaced"
    );
    assert_eq!(get(&executor, "k").await.as_deref(), Some("before-restart"));
}

#[tokio::test(flavor = "multi_thread")]
async fn entries_and_tags_persist_across_processes_and_pool_sizes() {
    let dir = temp_dir("persist");
    {
        let executor = start(1, Some(&dir), &[]);
        set(&executor, "kept", "persisted", "").await;
        set(&executor, "dropped", "stale", "gone").await;
        tokio::time::sleep(Duration::from_millis(2)).await;
        op(&executor, &[("op", "revalidate"), ("tags", "gone")]).await;
        executor.shutdown();
    }
    // A new executor with another pool size reads the same namespace.
    let executor = start(4, Some(&dir), &[]);
    assert_eq!(get(&executor, "kept").await.as_deref(), Some("persisted"));
    assert_eq!(get_tagged(&executor, "dropped", "gone").await, None);
    assert!(executor.metrics().cache.unwrap().persistent);
    executor.shutdown();
    // A different JavaScript environment is a different namespace.
    let executor = start(1, Some(&dir), &[("API_URL", "https://other.example")]);
    assert_eq!(get(&executor, "kept").await, None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_corrupted_entry_is_a_miss_and_is_removed() {
    let dir = temp_dir("corrupt");
    {
        let executor = start(1, Some(&dir), &[]);
        set(&executor, "k", "intact", "").await;
        executor.shutdown();
    }
    let files = entry_files(&dir);
    assert_eq!(files.len(), 1);
    let mut bytes = std::fs::read(&files[0]).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0x55;
    std::fs::write(&files[0], bytes).unwrap();
    let executor = start(1, Some(&dir), &[]);
    assert_eq!(get(&executor, "k").await, None);
    assert!(!files[0].exists());
    assert_eq!(executor.metrics().cache.unwrap().corrupt, 1);
    // The key works again.
    set(&executor, "k", "rewritten", "").await;
    assert_eq!(get(&executor, "k").await.as_deref(), Some("rewritten"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failing_disk_degrades_to_memory() {
    if unsafe_running_as_root() {
        eprintln!("skipped: permissions do not apply to root");
        return;
    }
    let dir = temp_dir("readonly");
    let executor = start(2, Some(&dir), &[]);
    set(&executor, "before", "on-disk", "").await;
    let ns = std::fs::read_dir(&dir)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let tmp = ns.join("tmp");
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o500)).unwrap();
    set(&executor, "after", "memory-only", "").await;
    assert_eq!(
        get(&executor, "after").await.as_deref(),
        Some("memory-only")
    );
    assert_eq!(get(&executor, "before").await.as_deref(), Some("on-disk"));
    assert!(executor.metrics().cache.unwrap().io_errors >= 1);
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o700)).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

fn unsafe_running_as_root() -> bool {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| {
            s.lines()
                .find(|l| l.starts_with("Uid:"))
                .map(|l| l.split_whitespace().nth(1) == Some("0"))
        })
        .unwrap_or(false)
}

#[tokio::test(flavor = "multi_thread")]
async fn javascript_cannot_read_cache_files_or_store_page_entries() {
    let dir = temp_dir("access");
    let executor = start(1, Some(&dir), &[]);
    set(&executor, "secret", "private-data", "").await;
    let file = entry_files(&dir).pop().unwrap();
    let r = op(
        &executor,
        &[("op", "read-file"), ("path", file.to_str().unwrap())],
    )
    .await;
    assert_eq!(r["error"]["code"], "EACCES", "{r}");
    // Keys never become paths: traversal-looking keys are just keys.
    set(&executor, "../../../../etc/passwd", "x", "").await;
    assert_eq!(
        get(&executor, "../../../../etc/passwd").await.as_deref(),
        Some("x")
    );
    assert!(entry_files(&dir).iter().all(|f| f.starts_with(&dir)));
    // Pages Router entries are not stored.
    let r = op(
        &executor,
        &[
            ("op", "set"),
            ("key", "page"),
            ("value", "html"),
            ("kind", "PAGES"),
        ],
    )
    .await;
    assert!(r.get("error").is_none());
    let r = op(
        &executor,
        &[("op", "get"), ("key", "page"), ("kind", "PAGES")],
    )
    .await;
    assert!(r["entry"].is_null());
    assert_eq!(executor.metrics().cache.unwrap().entries, 2);
    let _ = std::fs::remove_dir_all(&dir);
}

/// ISR entries (APP_PAGE) round-trip with their binary parts, are seen by
/// every isolate, survive a restart, and disappear when one of their tags
/// expires (judged by the handler with Next's rules).
#[tokio::test(flavor = "multi_thread")]
async fn page_entries_are_shared_persisted_and_tag_expired() {
    let dir = temp_dir("pages");
    {
        let executor = start(4, Some(&dir), &[]);
        let r = op(
            &executor,
            &[
                ("op", "set-page"),
                ("key", "/route-cache/APP_PAGE/x/$/p"),
                ("value", "<p>one</p>"),
                ("tags", "page-tag"),
            ],
        )
        .await;
        assert!(r.get("error").is_none(), "{r}");
        let readers: Vec<_> = (0..24)
            .map(|_| {
                let executor = executor.clone();
                tokio::spawn(async move {
                    op(
                        &executor,
                        &[("op", "get-page"), ("key", "/route-cache/APP_PAGE/x/$/p")],
                    )
                    .await
                })
            })
            .collect();
        for reader in readers {
            let r = reader.await.unwrap();
            assert_eq!(r["entry"]["html"], "<p>one</p>", "{r}");
            assert_eq!(r["entry"]["rsc"], "rsc:<p>one</p>");
            assert_eq!(r["entry"]["segment"], serde_json::json!([0, 255, 1]));
            assert_eq!(r["entry"]["revalidate"], 60);
        }
        assert_eq!(executor.metrics().cache.unwrap().response_sets, 1);
        executor.shutdown();
    }
    let executor = start(1, Some(&dir), &[]);
    let r = op(
        &executor,
        &[("op", "get-page"), ("key", "/route-cache/APP_PAGE/x/$/p")],
    )
    .await;
    assert_eq!(r["entry"]["html"], "<p>one</p>", "{r}");
    tokio::time::sleep(Duration::from_millis(2)).await;
    op(&executor, &[("op", "revalidate"), ("tags", "page-tag")]).await;
    let r = op(
        &executor,
        &[("op", "get-page"), ("key", "/route-cache/APP_PAGE/x/$/p")],
    )
    .await;
    assert!(r["entry"].is_null(), "{r}");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A blocking regeneration in one isolate makes another isolate's miss wait
/// for its result instead of rendering again.
#[tokio::test(flavor = "multi_thread")]
async fn a_missing_page_is_regenerated_once_across_isolates() {
    let executor = start(4, None, &[]);
    let key = "/route-cache/APP_PAGE/y/$/q";
    let first = {
        let executor = executor.clone();
        tokio::spawn(async move {
            op(
                &executor,
                &[
                    ("op", "lease"),
                    ("key", key),
                    ("then-set", "<p>regenerated</p>"),
                    ("hold", "300"),
                ],
            )
            .await
        })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    // Held by the first request: this one waits, then reads its result.
    let second = op(&executor, &[("op", "lease"), ("key", key)]).await;
    let first = first.await.unwrap();
    assert!(first["entry"].is_null(), "{first}");
    assert_eq!(second["entry"]["html"], "<p>regenerated</p>", "{second}");
    assert_ne!(first["isolate"], second["isolate"], "two isolates");
    assert!(second["waitedMs"].as_u64().unwrap() >= 100, "{second}");
    assert_eq!(executor.metrics().cache.unwrap().regeneration_waits, 1);
}
