//! Forwarded-header policies (--trusted-proxy, --strip-forwarded-headers)
//! with Next's real Server Action origin check: Next compares the action's
//! Origin with X-Forwarded-Host (or Host). Needs the fixture build:
//!
//!   cargo test -p nextrs-server --test proxy -- --ignored

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use nextrs_runtime::{Executor, ExecutorConfig};
use nextrs_server::{ClientAddr, ForwardedHeaders, ServerOptions};
use tower::ServiceExt;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/next-app")
}

fn who_am_i_id() -> String {
    let text =
        std::fs::read_to_string(fixture().join(".next/server/server-reference-manifest.json"))
            .unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&text).unwrap();
    manifest["node"]
        .as_object()
        .unwrap()
        .iter()
        .find(|(_, v)| v["exportedName"] == "whoAmI")
        .map(|(id, _)| id.clone())
        .expect("whoAmI action")
}

async fn app(executor: &Arc<Executor>, forwarded: ForwardedHeaders) -> Router {
    let info = nextrs_adapter::inspect(&fixture()).expect("fixture is built");
    nextrs_server::app(
        &info,
        &ServerOptions {
            executor: Some(executor.clone()),
            forwarded,
            ..ServerOptions::default()
        },
    )
    .await
    .unwrap()
}

/// A client-called Server Function, from `peer`, claiming `origin` and
/// optionally `x-forwarded-host`. Host is always the server's own.
async fn action(
    app: &Router,
    id: &str,
    peer: &str,
    origin: &str,
    forwarded_host: Option<&str>,
) -> StatusCode {
    let mut request = Request::post("/actions/client")
        .header("host", "127.0.0.1:3000")
        .header("origin", origin)
        .header("next-action", id)
        .header("accept", "text/x-component")
        .header("content-type", "text/plain;charset=UTF-8");
    if let Some(host) = forwarded_host {
        request = request
            .header("x-forwarded-host", host)
            .header("x-forwarded-proto", "http");
    }
    let mut request = request.body(Body::from(r#"["proxy-test"]"#)).unwrap();
    request
        .extensions_mut()
        .insert(ClientAddr(peer.parse::<SocketAddr>().unwrap()));
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let _ = to_bytes(response.into_body(), usize::MAX).await;
    status
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "requires fixtures/next-app to be built (npm ci && next build)"]
async fn forwarded_headers_are_trusted_only_from_configured_proxies() {
    let info = nextrs_adapter::inspect(&fixture()).expect("fixture is built");
    let executor = Executor::start(ExecutorConfig::new(
        info.project_dir.clone(),
        info.dist_dir.clone(),
    ))
    .expect("executor starts");
    let id = who_am_i_id();
    let direct = "203.0.113.7:50000";
    let proxy = "10.1.2.3:40000";

    // Default (next start parity): forwarded headers from anyone count.
    let pass = app(&executor, ForwardedHeaders::Pass).await;
    assert_eq!(
        action(&pass, &id, direct, "http://127.0.0.1:3000", None).await,
        StatusCode::OK,
        "direct, same origin"
    );
    assert_eq!(
        action(
            &pass,
            &id,
            direct,
            "http://evil.example",
            Some("evil.example")
        )
        .await,
        StatusCode::OK,
        "pass-through: a forged X-Forwarded-Host satisfies Next's check (the proxy must overwrite it)"
    );
    assert_eq!(
        action(&pass, &id, direct, "http://evil.example", None).await,
        StatusCode::INTERNAL_SERVER_ERROR,
        "invalid origin"
    );

    let trusted = app(
        &executor,
        ForwardedHeaders::TrustedProxies(vec!["10.0.0.0/8".parse().unwrap()]),
    )
    .await;
    // A reverse-proxied action: the proxy's public host is accepted.
    assert_eq!(
        action(
            &trusted,
            &id,
            proxy,
            "https://shop.example",
            Some("shop.example")
        )
        .await,
        StatusCode::OK,
        "trusted proxy"
    );
    // The same headers from anyone else are removed: the origin no longer
    // matches the host, and Next refuses the action.
    assert_eq!(
        action(
            &trusted,
            &id,
            direct,
            "https://shop.example",
            Some("shop.example")
        )
        .await,
        StatusCode::INTERNAL_SERVER_ERROR,
        "untrusted peer, forged X-Forwarded-Host"
    );
    // Direct same-origin requests still work; invalid origins still fail.
    assert_eq!(
        action(&trusted, &id, direct, "http://127.0.0.1:3000", None).await,
        StatusCode::OK,
        "direct"
    );
    assert_eq!(
        action(
            &trusted,
            &id,
            proxy,
            "http://evil.example",
            Some("shop.example")
        )
        .await,
        StatusCode::INTERNAL_SERVER_ERROR,
        "trusted proxy, wrong origin"
    );

    let strip = app(&executor, ForwardedHeaders::Strip).await;
    assert_eq!(
        action(
            &strip,
            &id,
            proxy,
            "https://shop.example",
            Some("shop.example")
        )
        .await,
        StatusCode::INTERNAL_SERVER_ERROR,
        "stripped"
    );
    assert_eq!(
        action(&strip, &id, direct, "http://127.0.0.1:3000", None).await,
        StatusCode::OK,
        "direct"
    );
    executor.shutdown();
}
