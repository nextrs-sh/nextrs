//! `/_next/image` end to end without Node: a copy of the default build
//! snapshot with an `images-manifest.json` (as Next 16.4.3 writes it with
//! its defaults) and generated public images. next start parity is covered
//! by tests/compat/image.mjs; these assert NextRS's own outcomes.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use nextrs_server::{ImageCacheOptions, ImageOptions, ImageStats, ServerOptions};
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
