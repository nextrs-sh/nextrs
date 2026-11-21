//! A Next.js 16.4.6 build with `deploymentId` (fixtures/next-app built with
//! `NEXT_DEPLOYMENT_ID=dpl-rc2-test`, snapshot in
//! tests/data/next-16.4.6-deployment-id). Next writes two routes-manifest
//! `onMatchHeaders` rules for it, which resolve-routes applies once a
//! route or file matched:
//!
//! - `/:path*` with `has: [{ type: header, key: rsc, value: 1 }]`, and
//! - `/_next/data/(.*)`,
//!
//! both setting `x-nextjs-deployment-id: dpl-rc2-test`. With a deployment
//! ID Next leaves the build ID (`b`) out of the RSC payload, and the client
//! router compares this header with its own deployment ID instead: without
//! it, every client navigation would turn into a full page load.
//!
//! Expected values marked "next start" were recorded from next start
//! 16.4.6 serving the same build with the same NEXT_DEPLOYMENT_ID.

use std::path::{Path, PathBuf};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, Response, StatusCode};
use nextrs_server::{ServerOptions, StartError};
use tower::ServiceExt;

const ID: &str = "dpl-rc2-test";
const HEADER: &str = "x-nextjs-deployment-id";
const SNAPSHOT: &str = "next-16.4.6-deployment-id";

fn snapshot(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/data")
        .join(name)
}

/// No executor here: the fixture's Server Actions and ISR pages are
/// unsupported without JavaScript; these tests read routing only.
fn options() -> ServerOptions {
    ServerOptions {
        allow_unsupported: true,
        ..ServerOptions::default()
    }
}

async fn app_for(dir: &Path) -> Router {
    let info = nextrs_adapter::inspect(dir).unwrap();
    nextrs_server::app(&info, &options()).await.unwrap()
}

async fn send(app: &Router, request: Request<Body>) -> (Response<Body>, Vec<u8>) {
    let response = app.clone().oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, usize::MAX).await.unwrap().to_vec();
    (Response::from_parts(parts, Body::empty()), bytes)
}

async fn request(
    app: &Router,
    method: Method,
    uri: &str,
    headers: &[(&str, &str)],
) -> (Response<Body>, Vec<u8>) {
    let mut builder = Request::builder().method(method).uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    send(app, builder.body(Body::empty()).unwrap()).await
}

fn deployment_header(response: &Response<Body>) -> Vec<&str> {
    response
        .headers()
        .get_all(HEADER)
        .iter()
        .map(|v| v.to_str().unwrap())
        .collect()
}

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(snapshot(SNAPSHOT).join(".next").join(rel)).unwrap()
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

/// A copy of the snapshot with its routes-manifest `onMatchHeaders`
/// replaced.
fn with_on_match_headers(label: &str, rules: serde_json::Value) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("nextrs-server-dpl-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    copy_dir(&snapshot(SNAPSHOT), &dir);
    let path = dir.join(".next/routes-manifest.json");
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    manifest["onMatchHeaders"] = rules;
    std::fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    dir
}

#[tokio::test]
async fn starts_with_the_rules_next_writes_for_a_deployment_id() {
    let info = nextrs_adapter::inspect(&snapshot(SNAPSHOT)).unwrap();
    assert_eq!(info.routing.on_match_headers.len(), 2);
    assert_eq!(info.config.deployment.id.as_deref(), Some(ID));
    assert!(!info.config.deployment.use_skew_cookie);
    assert!(!info.config.deployment.runtime_server_id);
    assert!(
        !info.diagnostics.iter().any(|d| d.code == "routing-rule"),
        "{:?}",
        info.diagnostics
    );
    // rc.1 refused: "routes-manifest onMatchHeaders are not implemented".
    let _app = nextrs_server::app(&info, &options())
        .await
        .expect("a deployment ID build starts");
}

/// next start: `rsc: 1` GET /?_rsc → 200 text/x-component,
/// x-nextjs-deployment-id: dpl-rc2-test, the prerendered payload.
#[tokio::test]
async fn rsc_requests_get_the_deployment_id() {
    let app = app_for(&snapshot(SNAPSHOT)).await;
    for name in ["rsc", "RSC"] {
        let (response, body) = request(&app, Method::GET, "/?_rsc", &[(name, "1")]).await;
        assert_eq!(response.status(), StatusCode::OK, "{name}");
        assert_eq!(deployment_header(&response), [ID], "{name}");
        assert_eq!(
            response.headers()["content-type"],
            "text/x-component",
            "{name}"
        );
        assert_eq!(body, read("server/app/index.rsc"));
    }
    // The payload itself carries no build ID: the header is the only
    // source the client router has.
    let payload = String::from_utf8(read("server/app/index.rsc")).unwrap();
    assert!(!payload.contains("\"b\":\""), "{}", &payload[..200]);
    // HEAD routes the same way.
    let (response, body) = request(&app, Method::HEAD, "/?_rsc", &[("rsc", "1")]).await;
    assert_eq!(deployment_header(&response), [ID]);
    assert!(body.is_empty());
}

/// next start: the `_rsc` cache-busting redirect of a matched route keeps
/// routing's headers (307, x-nextjs-deployment-id, Location /?_rsc).
#[tokio::test]
async fn the_rsc_cache_busting_redirect_carries_it() {
    let app = app_for(&snapshot(SNAPSHOT)).await;
    for uri in ["/", "/?_rsc=wrong"] {
        let (response, _) = request(&app, Method::GET, uri, &[("rsc", "1")]).await;
        assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT, "{uri}");
        assert_eq!(response.headers()["location"], "/?_rsc", "{uri}");
        assert_eq!(deployment_header(&response), [ID], "{uri}");
    }
}

/// next start: documents, `rsc: 2` and `rsc: 0` get no header (the rule's
/// `has` value must match exactly); the HTML carries the ID on <html>.
#[tokio::test]
async fn other_requests_do_not() {
    let app = app_for(&snapshot(SNAPSHOT)).await;
    let (response, body) = request(&app, Method::GET, "/", &[]).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(deployment_header(&response).is_empty());
    assert_eq!(body, read("server/app/index.html"));
    let html = String::from_utf8(body).unwrap();
    assert!(html.contains(&format!("<html data-dpl-id=\"{ID}\"")));
    assert!(html.contains(&format!(".js?dpl={ID}")));
    for value in ["2", "0", "", "1 "] {
        let (response, _) = request(&app, Method::GET, "/", &[("rsc", value)]).await;
        assert_eq!(response.status(), StatusCode::OK, "rsc: {value:?}");
        assert!(deployment_header(&response).is_empty(), "rsc: {value:?}");
    }
}

/// next start: a static file is a matched output too (rsc: 1 gets the
/// header), and the `?dpl=` token is not validated outside Next's test
/// mode: any value serves the file.
#[tokio::test]
async fn static_files() {
    let app = app_for(&snapshot(SNAPSHOT)).await;
    let path = "/_next/static/chunks/310vm2bl3xxpt.js";
    let file = read("static/chunks/310vm2bl3xxpt.js");
    for query in ["", &format!("?dpl={ID}"), "?dpl=other-deployment"] {
        let uri = format!("{path}{query}");
        let (response, body) = request(&app, Method::GET, &uri, &[]).await;
        assert_eq!(response.status(), StatusCode::OK, "{uri}");
        assert!(deployment_header(&response).is_empty(), "{uri}");
        assert_eq!(body, file, "{uri}");
    }
    let (response, _) = request(&app, Method::GET, path, &[("rsc", "1")]).await;
    assert_eq!(deployment_header(&response), [ID]);
}

/// resolve-routes applies onMatchHeaders for a `matchedOutput` only:
/// redirects and unmatched paths do not get them from routing. (next start
/// renders an unmatched RSC request with the not-found page's handler,
/// which sets the header itself; that part runs in JavaScript.)
#[tokio::test]
async fn redirects_and_unmatched_paths_get_none_from_routing() {
    let app = app_for(&snapshot(SNAPSHOT)).await;
    // The trailing-slash redirect (resHeaders: null).
    let (response, _) = request(&app, Method::GET, "/dynamic/", &[("rsc", "1")]).await;
    assert_eq!(response.status(), StatusCode::PERMANENT_REDIRECT);
    assert!(deployment_header(&response).is_empty());
    // Repeated slashes.
    let (response, _) = request(&app, Method::GET, "//dynamic", &[("rsc", "1")]).await;
    assert_eq!(response.status(), StatusCode::PERMANENT_REDIRECT);
    assert!(deployment_header(&response).is_empty());
    // No executor here: the static not-found page.
    let (response, _) = request(&app, Method::GET, "/no-such-page", &[]).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(deployment_header(&response).is_empty());
    // An App Router build has no /_next/data route: next start answers 404
    // without the header.
    let data = format!("/_next/data/{}/x.json", read_build_id());
    let (response, _) = request(&app, Method::GET, &data, &[]).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert!(deployment_header(&response).is_empty());
}

fn read_build_id() -> String {
    String::from_utf8(read("BUILD_ID")).unwrap()
}

/// A build without a deployment ID has no onMatchHeaders and sends no
/// header (next start, the 16.4.6 static-params snapshot).
#[tokio::test]
async fn builds_without_a_deployment_id_are_unchanged() {
    let dir = snapshot("next-16.4.6-static-params");
    let info = nextrs_adapter::inspect(&dir).unwrap();
    assert!(info.routing.on_match_headers.is_empty());
    assert_eq!(info.config.deployment.id, None);
    let app = app_for(&dir).await;
    let (response, _) = request(&app, Method::GET, "/?_rsc", &[("rsc", "1")]).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(deployment_header(&response).is_empty());
}

/// onMatchHeaders are generic header rules: `has` / `missing` of every
/// kind, parameters in values, and several rules in order, matched against
/// the path routing ended on.
#[tokio::test]
async fn generic_rules_follow_header_rule_semantics() {
    let dir = with_on_match_headers(
        "generic",
        serde_json::json!([
            {
                "source": "/:path*",
                "regex": "^(?:/((?:[^/]+?)(?:/(?:[^/]+?))*))?(?:/)?$",
                "has": [{ "type": "cookie", "key": "beta" }],
                "headers": [{ "key": "x-beta", "value": "on" }]
            },
            {
                "source": "/:path*",
                "regex": "^(?:/((?:[^/]+?)(?:/(?:[^/]+?))*))?(?:/)?$",
                "missing": [{ "type": "query", "key": "quiet" }],
                "headers": [{ "key": "x-loud", "value": "yes" }]
            },
            {
                "source": "/gsp/:slug",
                "regex": "^/gsp(?:/([^/]+?))(?:/)?$",
                "headers": [{ "key": "x-slug", "value": "slug-:slug" }]
            },
            {
                "source": "/:path*",
                "regex": "^(?:/((?:[^/]+?)(?:/(?:[^/]+?))*))?(?:/)?$",
                "headers": [{ "key": "x-loud", "value": "overridden" }]
            }
        ]),
    );
    let app = app_for(&dir).await;
    let value = |response: &Response<Body>, name: &str| {
        response
            .headers()
            .get(name)
            .map(|v| v.to_str().unwrap().to_owned())
    };
    let (response, _) = request(&app, Method::GET, "/", &[("cookie", "beta=1")]).await;
    assert_eq!(value(&response, "x-beta").as_deref(), Some("on"));
    // A later rule replaces an earlier value (resHeaders[key] = value).
    assert_eq!(value(&response, "x-loud").as_deref(), Some("overridden"));
    let (response, _) = request(&app, Method::GET, "/?quiet=1", &[]).await;
    assert_eq!(value(&response, "x-beta"), None);
    assert_eq!(value(&response, "x-loud").as_deref(), Some("overridden"));
    let (response, _) = request(&app, Method::GET, "/gsp/alpha", &[]).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(value(&response, "x-slug").as_deref(), Some("slug-alpha"));
    std::fs::remove_dir_all(&dir).unwrap();
}

/// What Next does not write, or writes differently, is refused at startup
/// instead of being served wrongly.
#[tokio::test]
async fn malformed_rules_refuse_to_start() {
    for (label, rules) in [
        (
            "destination",
            serde_json::json!([{
                "source": "/:path*",
                "regex": "^(?:/((?:[^/]+?)(?:/(?:[^/]+?))*))?(?:/)?$",
                "destination": "/elsewhere",
                "headers": [{ "key": HEADER, "value": ID }]
            }]),
        ),
        (
            "no-headers",
            serde_json::json!([{
                "source": "/:path*",
                "regex": "^(?:/((?:[^/]+?)(?:/(?:[^/]+?))*))?(?:/)?$"
            }]),
        ),
        (
            "bad-has",
            serde_json::json!([{
                "source": "/:path*",
                "regex": "^(?:/((?:[^/]+?)(?:/(?:[^/]+?))*))?(?:/)?$",
                "has": [{ "type": "header" }],
                "headers": [{ "key": HEADER, "value": ID }]
            }]),
        ),
        ("not-array", serde_json::json!({ "source": "/" })),
    ] {
        let dir = with_on_match_headers(label, rules);
        let info = nextrs_adapter::inspect(&dir).unwrap();
        assert!(
            info.diagnostics
                .iter()
                .any(|d| d.code == "routing-rule" && d.message.contains("onMatchHeaders")),
            "{label}: {:?}",
            info.diagnostics
        );
        match nextrs_server::app(&info, &ServerOptions::default()).await {
            Err(StartError::Unsupported(diagnostics)) => assert!(
                diagnostics.iter().any(|d| d.code == "routing-rule"),
                "{label}: {diagnostics:?}"
            ),
            Err(other) => panic!("{label}: unexpected error {other}"),
            Ok(_) => panic!("{label}: started with a malformed onMatchHeaders"),
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
    // A regex that does not compile, or whose groups do not fit its
    // source, is a routing error.
    let dir = with_on_match_headers(
        "regex",
        serde_json::json!([{
            "source": "/:path*",
            "regex": "^(?:/(",
            "headers": [{ "key": HEADER, "value": ID }]
        }]),
    );
    let info = nextrs_adapter::inspect(&dir).unwrap();
    match nextrs_server::app(&info, &options()).await {
        Err(StartError::Routing { .. }) => {}
        Err(other) => panic!("regex: unexpected error {other}"),
        Ok(_) => panic!("regex: started with an invalid regex"),
    }
    std::fs::remove_dir_all(&dir).unwrap();
}

/// base-server's deployment ID, which NextRS gives the JavaScript runtime
/// as process.env.NEXT_DEPLOYMENT_ID.
#[test]
fn server_deployment_id_follows_base_server() {
    use nextrs_adapter::DeploymentConfig;
    let config = |id: Option<&str>, skew: bool, runtime: bool| DeploymentConfig {
        id: id.map(str::to_owned),
        use_skew_cookie: skew,
        runtime_server_id: runtime,
        supports_immutable_assets: false,
    };
    assert_eq!(config(None, false, false).server_id(None).unwrap(), "");
    assert_eq!(config(Some(ID), false, false).server_id(None).unwrap(), ID);
    // next start overwrites the variable with the config's ID.
    assert_eq!(
        config(Some(ID), false, false)
            .server_id(Some("other"))
            .unwrap(),
        ID
    );
    assert_eq!(config(Some(ID), true, false).server_id(None).unwrap(), "");
    assert_eq!(
        config(Some(ID), false, true)
            .server_id(Some("runtime-id"))
            .unwrap(),
        "runtime-id"
    );
    for missing in [None, Some("")] {
        let error = config(None, false, true).server_id(missing).unwrap_err();
        assert!(error.contains("runtimeServerDeploymentId"), "{error}");
    }
}
