//! The `_rsc` cache-busting check on prerendered App Router output, and
//! `experimental.validateRSCRequestHeaders: false`, which turns it off in
//! next start's base server (route handlers and dynamic pages share the
//! same check: tests/dynamic.rs).

use std::path::{Path, PathBuf};

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use nextrs_server::ServerOptions;
use tower::ServiceExt;

fn snapshot() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/data/next-16.4.3-default")
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

async fn status_and_location(
    app: &axum::Router,
    uri: &str,
    headers: &[(&str, &str)],
) -> (StatusCode, Option<String>, Vec<u8>) {
    let mut request = Request::get(uri);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let location = response
        .headers()
        .get("location")
        .map(|v| v.to_str().unwrap().to_owned());
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
    (status, location, body.to_vec())
}

async fn app(dir: &Path) -> axum::Router {
    let info = nextrs_adapter::inspect(dir).unwrap();
    let options = ServerOptions {
        allow_unsupported: true,
        ..ServerOptions::default()
    };
    nextrs_server::app(&info, &options).await.unwrap()
}

#[tokio::test]
async fn rsc_requests_are_validated_by_default() {
    let app = app(&snapshot()).await;
    let (status, location, _) = status_and_location(&app, "/", &[("rsc", "1")]).await;
    assert_eq!(status, StatusCode::TEMPORARY_REDIRECT);
    assert_eq!(location.as_deref(), Some("/?_rsc"));
    let (status, _, body) = status_and_location(&app, "/?_rsc", &[("rsc", "1")]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body,
        std::fs::read(snapshot().join(".next/server/app/index.rsc")).unwrap()
    );
    // `rsc: 1` twice is not an RSC request (Node's "1, 1"): the document.
    let (status, location, body) =
        status_and_location(&app, "/", &[("rsc", "1"), ("rsc", "1")]).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(location, None);
    assert!(body.starts_with(b"<!DOCTYPE html>"));
}

#[tokio::test]
async fn validate_rsc_request_headers_false_skips_the_redirect() {
    let dir = std::env::temp_dir().join(format!("nextrs-rsc-novalidate-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&snapshot(), &dir);
    let path = dir.join(".next/required-server-files.json");
    let mut files: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    files["config"]["experimental"]["validateRSCRequestHeaders"] = false.into();
    std::fs::write(&path, serde_json::to_vec(&files).unwrap()).unwrap();
    let info = nextrs_adapter::inspect(&dir).unwrap();
    assert!(info.config.skip_rsc_header_validation);
    assert!(
        !nextrs_adapter::inspect(&snapshot())
            .unwrap()
            .config
            .skip_rsc_header_validation
    );
    let app = app(&dir).await;
    for uri in ["/", "/?_rsc=wrong"] {
        let (status, location, body) = status_and_location(&app, uri, &[("rsc", "1")]).await;
        assert_eq!(status, StatusCode::OK, "{uri}");
        assert_eq!(location, None, "{uri}");
        assert_eq!(
            body,
            std::fs::read(snapshot().join(".next/server/app/index.rsc")).unwrap()
        );
    }
    std::fs::remove_dir_all(&dir).unwrap();
}
