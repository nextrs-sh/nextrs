//! `/_next/image` end to end without Node: a copy of the default build
//! snapshot with an `images-manifest.json` (as Next 16.4.3 writes it with
//! its defaults) and generated public images. next start parity is covered
//! by tests/compat/image.mjs; these assert NextRS's own outcomes.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use nextrs_server::{
    ImageCacheOptions, ImageFault, ImageFaults, ImageOptions, ImageStats, ServerOptions,
};
use tower::ServiceExt;

static SEQ: AtomicU32 = AtomicU32::new(0);

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap().flatten() {
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A build with an image config and public/photos/{photo,alpha}.png.
fn project(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nextrs-image-test-{name}-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let snapshot =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/data/next-16.4.3-default");
    copy_dir(&snapshot, &dir);
    // Next 16.4.3's defaults, localPatterns compiled from `**`.
    let manifest = serde_json::json!({
        "version": 1,
        "images": {
            "deviceSizes": [640, 750, 828, 1080, 1200, 1920, 2048, 3840],
            "imageSizes": [32, 48, 64, 96, 128, 256, 384],
            "path": "/_next/image", "loader": "default", "loaderFile": "", "domains": [],
            "disableStaticImages": false, "minimumCacheTTL": 14400, "formats": ["image/webp"],
            "maximumRedirects": 3, "maximumResponseBody": 50000000, "dangerouslyAllowLocalIP": false,
            "dangerouslyAllowSVG": false,
            "contentSecurityPolicy": "script-src 'none'; frame-src 'none'; sandbox;",
            "contentDispositionType": "attachment",
            "localPatterns": [{ "pathname": "^(?:(?!(?:^|\\/)\\.{1,2}(?:\\/|$))(?:(?:(?!(?:^|\\/)\\.{1,2}(?:\\/|$)).)*?)\\/?)$", "search": "" }],
            "remotePatterns": [{ "protocol": "https", "hostname": "^(?:images\\.example\\.com)$", "pathname": "^(?:\\/.*)$" }],
            "qualities": [75], "unoptimized": false, "customCacheHandler": false
        }
    });
    std::fs::write(dir.join(".next/images-manifest.json"), manifest.to_string()).unwrap();
    let photos = dir.join("public/photos");
    std::fs::create_dir_all(&photos).unwrap();
    image::RgbImage::from_fn(800, 600, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, 90])
    })
    .save(photos.join("photo.png"))
    .unwrap();
    image::RgbaImage::from_fn(200, 100, |x, _| {
        image::Rgba([200, 30, 30, if x < 100 { 255 } else { 0 }])
    })
    .save(photos.join("alpha.png"))
    .unwrap();
    dir
}

async fn app(dir: &Path, images: ImageOptions) -> (Router, ImageStats) {
    let info = nextrs_adapter::inspect(dir).unwrap();
    let (router, stats) = nextrs_server::app_with_stats(
        &info,
        &ServerOptions {
            images,
            ..ServerOptions::default()
        },
    )
    .await
    .unwrap();
    (router, stats.expect("the build has an image config"))
}

struct Answer {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Vec<u8>,
}

async fn get(app: &Router, uri: &str, accept: &str) -> Answer {
    let request = Request::get(uri)
        .header("accept", accept)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    Answer {
        status: parts.status,
        headers: parts.headers,
        body: to_bytes(body, usize::MAX).await.unwrap().to_vec(),
    }
}

fn header<'a>(answer: &'a Answer, name: &str) -> &'a str {
    answer
        .headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
}

fn stat(stats: &ImageStats, name: &str) -> u64 {
    stats
        .snapshot()
        .into_iter()
        .find(|(k, _)| *k == name)
        .map_or(0, |(_, v)| v)
}

const PHOTO: &str = "/_next/image?url=%2Fphotos%2Fphoto.png&w=640&q=75";

#[tokio::test]
async fn optimizes_and_caches_a_public_image() {
    let dir = project("basic");
    let (app, stats) = app(&dir, ImageOptions::default()).await;
    let first = get(&app, PHOTO, "image/webp,*/*").await;
    assert_eq!(first.status, StatusCode::OK);
    assert_eq!(header(&first, "content-type"), "image/webp");
    assert_eq!(header(&first, "x-nextjs-cache"), "MISS");
    assert_eq!(
        header(&first, "cache-control"),
        "public, max-age=14400, must-revalidate"
    );
    assert_eq!(
        header(&first, "content-disposition"),
        "attachment; filename=\"photo.webp\""
    );
    let decoded = image::load_from_memory(&first.body).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (640, 480));
    let second = get(&app, PHOTO, "image/webp,*/*").await;
    assert_eq!(header(&second, "x-nextjs-cache"), "HIT");
    assert_eq!(second.body, first.body);
    // Another format is another entry.
    let png = get(&app, PHOTO, "*/*").await;
    assert_eq!(header(&png, "content-type"), "image/png");
    assert_eq!(header(&png, "x-nextjs-cache"), "MISS");
    assert_eq!(stat(&stats, "transforms"), 2);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn refuses_paths_outside_public_and_patterns() {
    let dir = project("access");
    // A symlink in public/ to a file outside it.
    std::fs::write(dir.join("secret.png"), b"not served").unwrap();
    std::os::unix::fs::symlink(dir.join("secret.png"), dir.join("public/photos/link.png")).unwrap();
    std::os::unix::fs::symlink(dir.join(".next"), dir.join("public/photos/next")).unwrap();
    let (app, stats) = app(&dir, ImageOptions::default()).await;
    for (url, status, body) in [
        // `..` is normalized away; `.next` is allowed by `**` (only `.`
        // and `..` segments are excluded) and is not served: 404.
        (
            "%2Fphotos%2F..%2F.next%2FBUILD_ID",
            404,
            "\"url\" parameter is valid but internal response is invalid",
        ),
        (
            "%2Fphotos%2F..%2F..%2Fsecret.png",
            404,
            "\"url\" parameter is valid but internal response is invalid",
        ),
        (
            "%2Fphotos%2F%252e%252e%2F.next%2FBUILD_ID",
            404,
            "\"url\" parameter is valid but internal response is invalid",
        ),
        (
            "%2Fphotos%2Flink.png",
            404,
            "\"url\" parameter is valid but internal response is invalid",
        ),
        (
            "%2Fphotos%2Fnext%2FBUILD_ID",
            404,
            "\"url\" parameter is valid but internal response is invalid",
        ),
        (
            "%2Fpackage.json",
            404,
            "\"url\" parameter is valid but internal response is invalid",
        ),
        (
            "%2F_next%2Fimage%3Furl%3D%252Fphotos%252Fphoto.png",
            400,
            "\"url\" parameter cannot be recursive",
        ),
        (
            "file%3A%2F%2F%2Fetc%2Fpasswd",
            400,
            "\"url\" parameter is invalid",
        ),
        (
            "https%3A%2F%2Fevil.example%2Fa.png",
            400,
            "\"url\" parameter is not allowed",
        ),
    ] {
        let answer = get(
            &app,
            &format!("/_next/image?url={url}&w=64&q=75"),
            "image/webp",
        )
        .await;
        assert_eq!(answer.status.as_u16(), status, "{url}");
        assert_eq!(String::from_utf8_lossy(&answer.body), body, "{url}");
    }
    assert_eq!(stat(&stats, "transforms"), 0);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn allowed_remote_image_without_network_policy_is_refused() {
    let dir = project("remote");
    let (app, _) = app(&dir, ImageOptions::default()).await;
    let answer = get(
        &app,
        "/_next/image?url=https%3A%2F%2Fimages.example.com%2Fa.png&w=64&q=75",
        "image/webp",
    )
    .await;
    assert_eq!(answer.status, StatusCode::BAD_REQUEST);
    assert_eq!(answer.body, b"\"url\" parameter is not allowed");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn concurrent_requests_share_one_transformation() {
    let dir = project("coalesce");
    let (app, stats) = app(&dir, ImageOptions::default()).await;
    let uri = "/_next/image?url=%2Fphotos%2Fphoto.png&w=1920&q=75";
    let answers =
        futures_util::future::join_all((0..100).map(|_| get(&app, uri, "image/webp"))).await;
    assert!(answers.iter().all(|a| a.status == StatusCode::OK));
    assert!(answers.iter().all(|a| a.body == answers[0].body));
    assert_eq!(stat(&stats, "transforms"), 1, "{:?}", stats.snapshot());
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn overload_answers_503() {
    let dir = project("overload");
    let (app, stats) = app(
        &dir,
        ImageOptions {
            concurrency: 1,
            max_queued: 0,
            ..ImageOptions::default()
        },
    )
    .await;
    let uris: Vec<String> = [640, 750, 828, 1080, 1200, 1920]
        .iter()
        .map(|w| format!("/_next/image?url=%2Fphotos%2Fphoto.png&w={w}&q=75"))
        .collect();
    let answers =
        futures_util::future::join_all(uris.iter().map(|u| get(&app, u, "image/webp"))).await;
    let ok = answers
        .iter()
        .filter(|a| a.status == StatusCode::OK)
        .count();
    let busy = answers
        .iter()
        .filter(|a| a.status == StatusCode::SERVICE_UNAVAILABLE)
        .count();
    assert!(
        ok >= 1 && busy >= 1 && ok + busy == answers.len(),
        "ok {ok} busy {busy}"
    );
    assert!(
        answers
            .iter()
            .filter(|a| a.status == StatusCode::SERVICE_UNAVAILABLE)
            .all(|a| header(a, "retry-after") == "1")
    );
    assert_eq!(stat(&stats, "overloaded") as usize, busy);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn too_many_pixels_serves_the_source() {
    let dir = project("pixels");
    let (app, stats) = app(
        &dir,
        ImageOptions {
            max_input_pixels: 1000,
            ..ImageOptions::default()
        },
    )
    .await;
    let answer = get(&app, PHOTO, "image/webp").await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(header(&answer, "content-type"), "image/png");
    assert_eq!(
        answer.body,
        std::fs::read(dir.join("public/photos/photo.png")).unwrap()
    );
    assert_eq!(
        header(&answer, "cache-control"),
        "public, max-age=14400, must-revalidate"
    );
    assert_eq!(stat(&stats, "fallbacks"), 1);
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn disk_cache_survives_a_restart() {
    let dir = project("disk");
    let cache_dir = dir.with_extension("cache");
    let options = || ImageOptions {
        cache: ImageCacheOptions {
            dir: Some(cache_dir.clone()),
            ..ImageCacheOptions::default()
        },
        ..ImageOptions::default()
    };
    let (first_app, _) = app(&dir, options()).await;
    let first = get(&first_app, PHOTO, "image/webp").await;
    assert_eq!(header(&first, "x-nextjs-cache"), "MISS");
    drop(first_app);
    let (second_app, stats) = app(&dir, options()).await;
    let second = get(&second_app, PHOTO, "image/webp").await;
    assert_eq!(header(&second, "x-nextjs-cache"), "HIT");
    assert_eq!(second.body, first.body);
    assert_eq!(stat(&stats, "transforms"), 0);
    assert_eq!(stat(&stats, "cache_disk_hits"), 1);
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(cache_dir);
}

#[tokio::test]
async fn alpha_survives_webp() {
    let dir = project("alpha");
    let (app, _) = app(&dir, ImageOptions::default()).await;
    let answer = get(
        &app,
        "/_next/image?url=%2Fphotos%2Falpha.png&w=64&q=75",
        "image/webp",
    )
    .await;
    let decoded = image::load_from_memory(&answer.body).unwrap().to_rgba8();
    assert_eq!(decoded.dimensions(), (64, 32));
    assert!(decoded.get_pixel(5, 16)[3] > 250);
    assert!(decoded.get_pixel(60, 16)[3] < 5);
    let _ = std::fs::remove_dir_all(dir);
}

/// Clients that leave while their transformation waits for a thread
/// cancel it and give their queue place back (a leaked place would turn
/// every later request into a 503).
#[tokio::test]
async fn cancelled_waits_leave_the_queue() {
    let dir = project("cancel");
    let (app, stats) = app(
        &dir,
        ImageOptions {
            concurrency: 1,
            max_queued: 4,
            ..ImageOptions::default()
        },
    )
    .await;
    for round in 0..3 {
        let requests: Vec<_> = [640, 750, 828, 1080, 1200, 1920]
            .iter()
            .map(|w| {
                let app = app.clone();
                let uri = format!("/_next/image?url=%2Fphotos%2Fphoto.png&w={w}&q=75");
                tokio::spawn(async move { get(&app, &uri, "image/png").await })
            })
            .collect();
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
        for r in &requests {
            r.abort();
        }
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        assert_eq!(
            stat(&stats, "queued"),
            0,
            "round {round}: {:?}",
            stats.snapshot()
        );
    }
    // Still served: no place was leaked.
    let answer = get(
        &app,
        "/_next/image?url=%2Fphotos%2Fphoto.png&w=256&q=75",
        "image/webp",
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    let _ = std::fs::remove_dir_all(dir);
}

/// A cache directory that cannot be written (a full or read-only disk):
/// images are still served, from memory, and the failures are counted.
#[tokio::test]
async fn unwritable_cache_directory_still_serves() {
    use std::os::unix::fs::PermissionsExt;
    let dir = project("readonly");
    let cache_dir = dir.with_extension("cache");
    let (app, stats) = app(
        &dir,
        ImageOptions {
            cache: ImageCacheOptions {
                dir: Some(cache_dir.clone()),
                ..ImageCacheOptions::default()
            },
            ..ImageOptions::default()
        },
    )
    .await;
    std::fs::set_permissions(&cache_dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    let first = get(&app, PHOTO, "image/webp").await;
    let second = get(&app, PHOTO, "image/webp").await;
    std::fs::set_permissions(&cache_dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(first.status, StatusCode::OK);
    assert_eq!(header(&second, "x-nextjs-cache"), "HIT");
    // Root ignores directory permissions; the check needs a normal user.
    if unsafe_euid() != 0 {
        assert_eq!(stat(&stats, "cache_write_errors"), 1);
    }
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(cache_dir);
}

fn unsafe_euid() -> u32 {
    std::fs::metadata("/proc/self").map_or(1, |m| std::os::unix::fs::MetadataExt::uid(&m))
}

// Memory-weighted admission (Phase 2E): reservations from the source's
// header estimate, held until the work ends, released on every outcome.

fn faults(f: impl Fn(&str) -> ImageFault + Send + Sync + 'static) -> ImageFaults {
    ImageFaults {
        before_transform: Some(std::sync::Arc::new(f)),
    }
}

/// Polls a counter until it has `want` (the work runs on its own thread).
async fn settle(stats: &ImageStats, name: &str, want: u64) {
    for _ in 0..200 {
        if stat(stats, name) == want {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
    }
    panic!(
        "{name} stayed {} (want {want}): {:?}",
        stat(stats, name),
        stats.snapshot()
    );
}

fn estimate(dir: &Path, file: &str, width: u32) -> u64 {
    let bytes = std::fs::read(dir.join("public/photos").join(file)).unwrap();
    nextrs_image::estimate(&bytes, nextrs_image::ImageType::Webp, width).unwrap()
}

fn photo(width: u32) -> String {
    format!("/_next/image?url=%2Fphotos%2Fphoto.png&w={width}&q=75")
}

async fn all_released(stats: &ImageStats) {
    settle(stats, "memory_reserved_bytes", 0).await;
    settle(stats, "source_held_bytes", 0).await;
    settle(stats, "queued", 0).await;
    settle(stats, "in_flight", 0).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn small_images_proceed_side_by_side_within_the_budget() {
    let dir = project("admit-small");
    let (app, stats) = app(
        &dir,
        ImageOptions {
            concurrency: 4,
            memory_budget: 64 * 1024 * 1024,
            faults: faults(|_| ImageFault::Sleep(std::time::Duration::from_millis(200))),
            ..ImageOptions::default()
        },
    )
    .await;
    let started = std::time::Instant::now();
    let uris = [32, 48, 64, 96].map(photo);
    let answers =
        futures_util::future::join_all(uris.iter().map(|u| get(&app, u, "image/webp"))).await;
    let elapsed = started.elapsed();
    assert!(answers.iter().all(|a| a.status == StatusCode::OK));
    assert!(
        answers
            .iter()
            .all(|a| header(a, "content-type") == "image/webp")
    );
    // Four 200 ms transformations side by side, not one after another.
    assert!(
        elapsed < std::time::Duration::from_millis(700),
        "{elapsed:?}"
    );
    assert_eq!(stat(&stats, "memory_waits"), 0);
    all_released(&stats).await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn large_work_waits_for_memory_and_never_exceeds_the_budget() {
    let dir = project("admit-large");
    let widths = [640, 750, 828, 1080];
    let largest = widths
        .iter()
        .map(|&w| estimate(&dir, "photo.png", w))
        .max()
        .unwrap();
    // Room for one of them at a time, with four threads free.
    let budget = largest + largest / 2;
    let (app, stats) = app(
        &dir,
        ImageOptions {
            concurrency: 4,
            memory_budget: budget,
            faults: faults(|_| ImageFault::Sleep(std::time::Duration::from_millis(150))),
            ..ImageOptions::default()
        },
    )
    .await;
    let peak = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));
    let sampling = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let sampler = tokio::spawn({
        let (stats, peak, sampling) = (stats.clone(), peak.clone(), sampling.clone());
        async move {
            while sampling.load(Ordering::Relaxed) {
                peak.fetch_max(stat(&stats, "memory_reserved_bytes"), Ordering::Relaxed);
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        }
    });
    let uris = widths.map(photo);
    let answers =
        futures_util::future::join_all(uris.iter().map(|u| get(&app, u, "image/webp"))).await;
    sampling.store(false, Ordering::Relaxed);
    sampler.await.unwrap();
    assert!(answers.iter().all(|a| a.status == StatusCode::OK));
    assert!(
        answers
            .iter()
            .all(|a| header(a, "content-type") == "image/webp")
    );
    assert!(stat(&stats, "memory_waits") >= 3, "{:?}", stats.snapshot());
    let peak = peak.load(Ordering::Relaxed);
    assert!(peak > 0 && peak <= budget, "peak {peak} budget {budget}");
    all_released(&stats).await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn small_work_runs_beside_large_work_when_it_fits() {
    let dir = project("admit-mixed");
    let large = estimate(&dir, "photo.png", 1080);
    let small = [32, 48, 64]
        .iter()
        .map(|&w| estimate(&dir, "alpha.png", w))
        .max()
        .unwrap();
    let (app, stats) = app(
        &dir,
        ImageOptions {
            concurrency: 4,
            // Reservations round up to KiB.
            memory_budget: large + 3 * small + 64 * 1024,
            faults: faults(|_| ImageFault::Sleep(std::time::Duration::from_millis(150))),
            ..ImageOptions::default()
        },
    )
    .await;
    let mut uris = vec![photo(1080)];
    uris.extend([32, 48, 64].map(|w| format!("/_next/image?url=%2Fphotos%2Falpha.png&w={w}&q=75")));
    let answers =
        futures_util::future::join_all(uris.iter().map(|u| get(&app, u, "image/webp"))).await;
    assert!(answers.iter().all(|a| a.status == StatusCode::OK));
    // Nobody waited for memory: the small ones ran beside the large one.
    assert_eq!(stat(&stats, "memory_waits"), 0);
    all_released(&stats).await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn cancellation_releases_memory_while_waiting_and_after_running() {
    let dir = project("admit-cancel");
    let (app, stats) = app(
        &dir,
        ImageOptions {
            concurrency: 1,
            max_queued: 8,
            memory_budget: estimate(&dir, "photo.png", 1200) * 2,
            faults: faults(|_| ImageFault::Sleep(std::time::Duration::from_millis(300))),
            ..ImageOptions::default()
        },
    )
    .await;
    // Distinct keys every round (a finished one is cached).
    for widths in [
        [640, 750, 828, 1080, 1200],
        [1920, 2048, 3840, 32, 48],
        [64, 96, 128, 256, 384],
    ] {
        let requests: Vec<_> = widths
            .iter()
            .map(|&w| {
                let app = app.clone();
                tokio::spawn(async move { get(&app, &photo(w), "image/webp").await })
            })
            .collect();
        tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        // Some wait for memory, some for the thread, one runs.
        assert!(stat(&stats, "memory_reserved_bytes") > 0);
        for r in &requests {
            r.abort();
        }
        // The waiters leave at once; the running one when its work ends.
        all_released(&stats).await;
    }
    assert!(stat(&stats, "cancelled") >= 3);
    let answer = get(
        &app,
        "/_next/image?url=%2Fphotos%2Falpha.png&w=64&q=75",
        "image/webp",
    )
    .await;
    assert_eq!(answer.status, StatusCode::OK);
    all_released(&stats).await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn decoder_encoder_and_panic_failures_release_memory() {
    let dir = project("admit-fail");
    let (app, stats) = app(
        &dir,
        ImageOptions {
            faults: faults(|href| match href {
                h if h.ends_with("photo.png") => ImageFault::DecodeError,
                h if h.ends_with("alpha.png") => ImageFault::EncodeError,
                _ => ImageFault::None,
            }),
            ..ImageOptions::default()
        },
    )
    .await;
    let decode = get(&app, &photo(640), "image/webp").await;
    let encode = get(
        &app,
        "/_next/image?url=%2Fphotos%2Falpha.png&w=64&q=75",
        "image/webp",
    )
    .await;
    for answer in [&decode, &encode] {
        // next start serves the source unchanged when sharp fails.
        assert_eq!(answer.status, StatusCode::OK);
        assert_eq!(header(answer, "content-type"), "image/png");
    }
    assert_eq!(stat(&stats, "fallbacks"), 2);
    all_released(&stats).await;
    // A real decoder failure: a PNG cut in half.
    let png = std::fs::read(dir.join("public/photos/photo.png")).unwrap();
    std::fs::write(dir.join("public/photos/cut.png"), &png[..png.len() / 2]).unwrap();
    let (app, stats) = app_panicking(&dir).await;
    let cut = get(
        &app,
        "/_next/image?url=%2Fphotos%2Fcut.png&w=64&q=75",
        "image/webp",
    )
    .await;
    assert_eq!(cut.status, StatusCode::OK);
    assert_eq!(header(&cut, "content-type"), "image/png");
    let panicked = get(&app, &photo(64), "image/webp").await;
    assert_eq!(panicked.status, StatusCode::OK);
    assert_eq!(header(&panicked, "content-type"), "image/png");
    all_released(&stats).await;
    // Repeated requests after failures are still served.
    let again = get(
        &app,
        "/_next/image?url=%2Fphotos%2Falpha.png&w=64&q=75",
        "image/webp",
    )
    .await;
    assert_eq!(header(&again, "content-type"), "image/webp");
    all_released(&stats).await;
    let _ = std::fs::remove_dir_all(dir);
}

/// A fresh server whose photo.png transformations panic.
async fn app_panicking(dir: &Path) -> (Router, ImageStats) {
    app(
        dir,
        ImageOptions {
            faults: faults(|href| {
                if href.ends_with("photo.png") {
                    ImageFault::Panic
                } else {
                    ImageFault::None
                }
            }),
            ..ImageOptions::default()
        },
    )
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_timed_out_transformation_holds_its_memory_until_it_ends() {
    let dir = project("admit-timeout");
    let (app, stats) = app(
        &dir,
        ImageOptions {
            timeout: Some(std::time::Duration::from_millis(100)),
            faults: faults(|href| {
                if href.ends_with("photo.png") {
                    ImageFault::Sleep(std::time::Duration::from_millis(800))
                } else {
                    ImageFault::None
                }
            }),
            ..ImageOptions::default()
        },
    )
    .await;
    let started = std::time::Instant::now();
    let answer = get(&app, &photo(640), "image/webp").await;
    assert!(started.elapsed() < std::time::Duration::from_millis(600));
    assert_eq!(header(&answer, "content-type"), "image/png");
    // Answered, but the thread still works: its memory is still reserved.
    assert!(stat(&stats, "memory_reserved_bytes") > 0);
    all_released(&stats).await;
    let next = get(
        &app,
        "/_next/image?url=%2Fphotos%2Falpha.png&w=64&q=75",
        "image/webp",
    )
    .await;
    assert_eq!(header(&next, "content-type"), "image/webp");
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn work_larger_than_the_whole_budget_serves_the_source() {
    let dir = project("admit-reject");
    let (app, stats) = app(
        &dir,
        ImageOptions {
            memory_budget: estimate(&dir, "photo.png", 640) - 64 * 1024,
            ..ImageOptions::default()
        },
    )
    .await;
    let answer = get(&app, &photo(640), "image/webp").await;
    assert_eq!(answer.status, StatusCode::OK);
    assert_eq!(header(&answer, "content-type"), "image/png");
    assert_eq!(stat(&stats, "memory_rejected"), 1);
    assert_eq!(stat(&stats, "transforms"), 0);
    // A smaller output of the same source fits.
    let small = get(&app, &photo(64), "image/webp").await;
    assert_eq!(header(&small, "content-type"), "image/webp");
    all_released(&stats).await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_saturated_queue_answers_503_and_recovers() {
    let dir = project("admit-saturate");
    let (app, stats) = app(
        &dir,
        ImageOptions {
            concurrency: 1,
            max_queued: 1,
            faults: faults(|_| ImageFault::Sleep(std::time::Duration::from_millis(200))),
            ..ImageOptions::default()
        },
    )
    .await;
    let uris = [640, 750, 828, 1080, 1200, 1920].map(photo);
    let answers =
        futures_util::future::join_all(uris.iter().map(|u| get(&app, u, "image/webp"))).await;
    let ok = answers
        .iter()
        .filter(|a| a.status == StatusCode::OK)
        .count();
    let busy = answers
        .iter()
        .filter(|a| a.status == StatusCode::SERVICE_UNAVAILABLE)
        .count();
    assert_eq!((ok, busy), (2, 4));
    all_released(&stats).await;
    let after = get(&app, &photo(256), "image/webp").await;
    assert_eq!(after.status, StatusCode::OK);
    all_released(&stats).await;
    let _ = std::fs::remove_dir_all(dir);
}

#[tokio::test]
async fn the_source_budget_refuses_sources_it_cannot_hold() {
    let dir = project("admit-sources");
    let size = std::fs::metadata(dir.join("public/photos/photo.png"))
        .unwrap()
        .len();
    let (app, stats) = app(
        &dir,
        ImageOptions {
            source_budget: size - 1,
            ..ImageOptions::default()
        },
    )
    .await;
    let answer = get(&app, &photo(640), "image/webp").await;
    assert_eq!(answer.status, StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(stat(&stats, "source_rejected"), 1);
    all_released(&stats).await;
    // A smaller source fits.
    let small = get(
        &app,
        "/_next/image?url=%2Fphotos%2Falpha.png&w=64&q=75",
        "image/webp",
    )
    .await;
    assert_eq!(small.status, StatusCode::OK);
    all_released(&stats).await;
    let _ = std::fs::remove_dir_all(dir);
}
