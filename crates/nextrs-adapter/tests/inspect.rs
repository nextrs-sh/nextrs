//! Runs the reader against snapshots of real `next build` output
//! (tests/data, produced by scripts/snapshot-build.mjs from fixtures/next-app
//! with Next.js 16.4.3).

use std::path::{Path, PathBuf};

use nextrs_adapter::{
    InspectError, InspectOptions, Matcher, MetadataSource, Rendering, RouteKind, Runtime, Severity,
    inspect, inspect_with,
};

fn snapshot(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/data")
        .join(name)
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

/// A writable copy of a snapshot in a fresh temp directory.
fn scratch_copy(name: &str, label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nextrs-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&snapshot(name), &dir);
    dir
}

fn route_summary(info: &nextrs_adapter::BuildInfo) -> Vec<(String, RouteKind, Rendering)> {
    info.routes
        .iter()
        .map(|r| (r.page.clone(), r.kind, r.rendering.clone()))
        .collect()
}

fn expected_routes() -> Vec<(String, RouteKind, Rendering)> {
    [
        ("/", RouteKind::AppPage, Rendering::Static),
        ("/_global-error", RouteKind::AppPage, Rendering::Static),
        ("/_not-found", RouteKind::AppPage, Rendering::Static),
        ("/api/health", RouteKind::AppRoute, Rendering::Dynamic),
        ("/books/[id]", RouteKind::AppPage, Rendering::Dynamic),
        ("/dynamic", RouteKind::AppPage, Rendering::Dynamic),
        ("/request-info", RouteKind::AppPage, Rendering::Dynamic),
    ]
    .into_iter()
    .map(|(page, kind, rendering)| (page.to_owned(), kind, rendering))
    .collect()
}

#[test]
fn default_build_is_read_from_internal_manifests() {
    let info = inspect(&snapshot("next-16.4.3-default")).unwrap();
    assert_eq!(info.source, MetadataSource::InternalManifests);
    assert_eq!(info.build_id, "ihLjhVTae49O4KfRbiGbj");
    let version = info.next_version.as_ref().unwrap();
    assert_eq!(version.version, "16.4.3");
    assert_eq!(version.source, ".next/diagnostics/framework.json");
    assert!(info.routers.app);
    assert!(!info.routers.pages);
    assert_eq!(route_summary(&info), expected_routes());
    assert!(info.routes.iter().all(|r| r.runtime == Runtime::Nodejs));
    assert!(!info.has_unsupported(), "{:#?}", info.diagnostics);

    let home = info.prerender("/").unwrap();
    assert_eq!(home.status, 200);
    assert_eq!(home.revalidate, None);
    assert!(
        home.html
            .as_ref()
            .unwrap()
            .file
            .ends_with("server/app/index.html")
    );
    assert!(
        home.rsc
            .as_ref()
            .unwrap()
            .file
            .ends_with("server/app/index.rsc")
    );
    let segments: Vec<&str> = home.segments.iter().map(|(s, _)| s.as_str()).collect();
    assert_eq!(segments, ["/_tree", "/_full", "/__PAGE__"]);
    assert!(
        home.html
            .as_ref()
            .unwrap()
            .headers
            .contains(&("x-nextjs-stale-time".into(), "300".into()))
    );
    assert_eq!(info.prerender("/_not-found").unwrap().status, 404);
}

#[test]
fn adapter_build_is_read_from_adapter_output() {
    let info = inspect(&snapshot("next-16.4.3-adapter")).unwrap();
    assert_eq!(info.source, MetadataSource::AdapterApi);
    assert_eq!(
        info.next_version.as_ref().unwrap().source,
        "nextrs/build-output.json"
    );
    assert_eq!(route_summary(&info), expected_routes());

    // With an adapter configured, Next.js stores prerenders under
    // server/route-cache/APP_PAGE/<sha256(app path)>/$<name>.
    let html = &info.prerender("/").unwrap().html.as_ref().unwrap().file;
    assert!(
        html.to_string_lossy().ends_with(
            "server/route-cache/APP_PAGE/c6377dd9ce6a1c2cd100eb4344e843eb4639b9ed26f84643f77680e9eb454c4b/$/index.html"
        ),
        "{}",
        html.display()
    );
    // The adapter API omits /_not-found; it is filled in from the manifests.
    assert!(
        info.diagnostics
            .iter()
            .any(|d| d.code == "adapter-missing-prerender" && d.message.contains("/_not-found"))
    );
    assert_eq!(info.prerender("/_not-found").unwrap().status, 404);
}

#[test]
fn adapter_and_internal_sources_agree() {
    let path = snapshot("next-16.4.3-adapter");
    let adapter = inspect(&path).unwrap();
    let internal = inspect_with(
        &path,
        &InspectOptions {
            ignore_adapter_output: true,
        },
    )
    .unwrap();
    assert_eq!(internal.source, MetadataSource::InternalManifests);
    assert_eq!(route_summary(&adapter), route_summary(&internal));
    for (a, b) in adapter.routes.iter().zip(&internal.routes) {
        assert_eq!(a.entry, b.entry, "{}", a.page);
    }
    assert_eq!(adapter.prerenders.len(), internal.prerenders.len());
    for (a, b) in adapter.prerenders.iter().zip(&internal.prerenders) {
        assert_eq!(a.pathname, b.pathname);
        assert_eq!(a.status, b.status, "{}", a.pathname);
        let html = |p: &nextrs_adapter::Prerender| p.html.as_ref().map(|h| h.file.clone());
        assert_eq!(html(a), html(b), "{}", a.pathname);
        let mut a_segments: Vec<_> = a.segments.iter().map(|(s, r)| (s, &r.file)).collect();
        let mut b_segments: Vec<_> = b.segments.iter().map(|(s, r)| (s, &r.file)).collect();
        a_segments.sort();
        b_segments.sort();
        assert_eq!(a_segments, b_segments, "{}", a.pathname);
        for ((_, ra), (_, rb)) in a.segments.iter().zip(&b.segments) {
            let mut ha = ra.headers.clone();
            let mut hb = rb.headers.clone();
            ha.sort();
            hb.sort();
            assert_eq!(ha, hb, "segment headers for {}", a.pathname);
        }
    }
    assert_eq!(adapter.rsc.vary_header, internal.rsc.vary_header);
}

#[test]
fn dynamic_route_matcher_comes_from_build_metadata() {
    for name in ["next-16.4.3-default", "next-16.4.3-adapter"] {
        let info = inspect(&snapshot(name)).unwrap();
        let Matcher::Pattern { regex, params } = &info.route("/books/[id]").unwrap().matcher else {
            panic!("expected a pattern matcher in {name}");
        };
        assert_eq!(params, &["nxtPid"]);
        let regex = regex::Regex::new(regex).unwrap();
        assert!(regex.is_match("/books/42"));
        assert!(!regex.is_match("/books/42/extra"));
        assert!(!regex.is_match("/books"));
    }
}

#[test]
fn accepts_the_dist_directory_itself() {
    let info = inspect(&snapshot("next-16.4.3-default").join(".next")).unwrap();
    assert_eq!(info.build_id, "ihLjhVTae49O4KfRbiGbj");
}

#[test]
fn missing_build_output_is_an_error() {
    let dir = std::env::temp_dir().join(format!("nextrs-empty-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    match inspect(&dir) {
        Err(InspectError::NoBuildOutput { dist_dir }) => assert!(dist_dir.ends_with(".next")),
        other => panic!("expected NoBuildOutput, got {other:?}"),
    }
}

#[test]
fn stale_adapter_output_is_ignored() {
    let dir = scratch_copy("next-16.4.3-adapter", "stale");
    std::fs::write(dir.join(".next/BUILD_ID"), "a-newer-build").unwrap();
    let info = inspect(&dir).unwrap();
    assert_eq!(info.source, MetadataSource::InternalManifests);
    assert!(
        info.diagnostics
            .iter()
            .any(|d| d.code == "stale-adapter-output")
    );
}

#[test]
fn features_that_change_responses_are_flagged() {
    let dir = scratch_copy("next-16.4.3-default", "features");
    let dist = dir.join(".next");
    let edit_json = |rel: &str, edit: &dyn Fn(&mut serde_json::Value)| {
        let path = dist.join(rel);
        let mut json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        edit(&mut json);
        std::fs::write(&path, serde_json::to_vec(&json).unwrap()).unwrap();
    };
    edit_json("routes-manifest.json", &|m| {
        m["headers"] = serde_json::json!([{ "source": "/(.*)", "headers": [{ "key": "x-frame-options", "value": "DENY" }], "regex": "^/(.*)$" }]);
        m["redirects"].as_array_mut().unwrap().push(
            serde_json::json!({ "source": "/old", "destination": "/", "statusCode": 308, "regex": "^/old$" }),
        );
        m["rewrites"]["beforeFiles"] =
            serde_json::json!([{ "source": "/a", "destination": "/b", "regex": "^/a$" }]);
    });
    edit_json("server/middleware-manifest.json", &|m| {
        m["middleware"]["/"] =
            serde_json::json!({ "files": [], "name": "middleware", "page": "/" });
    });
    edit_json("server/server-reference-manifest.json", &|m| {
        m["node"]["abc123"] = serde_json::json!({ "workers": {} });
    });
    edit_json("prerender-manifest.json", &|m| {
        m["routes"]["/"]["initialRevalidateSeconds"] = serde_json::json!(60);
    });

    let info = inspect(&dir).unwrap();
    let unsupported: Vec<&str> = info
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Unsupported)
        .map(|d| d.code)
        .collect();
    for code in [
        "middleware",
        "custom-headers",
        "custom-redirects",
        "rewrites",
        "server-actions",
        "isr",
    ] {
        assert!(
            unsupported.contains(&code),
            "missing {code} in {unsupported:?}"
        );
    }
    assert_eq!(
        info.route("/").unwrap().rendering,
        Rendering::Revalidated { seconds: 60 }
    );
}
