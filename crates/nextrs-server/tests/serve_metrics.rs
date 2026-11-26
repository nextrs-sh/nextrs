//! Serving-stage metrics (`nextrs_http_served_total`, routing and head
//! timings, streamed-compression counters) on real Next.js build output:
//! each request is counted once, by what answered it, and nothing in the
//! output carries a path or another unbounded value.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode};
use nextrs_server::{HttpMetrics, ServerOptions};
use tower::ServiceExt;

fn snapshot(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/data")
        .join(name)
}

async fn app_with(metrics: &Arc<HttpMetrics>) -> Router {
    let info = nextrs_adapter::inspect(&snapshot("next-16.4.6-deployment-id")).unwrap();
    let options = ServerOptions {
        allow_unsupported: true,
        metrics: metrics.clone(),
        ..ServerOptions::default()
    };
    nextrs_server::app(&info, &options).await.unwrap()
}

async fn get(app: &Router, uri: &str, headers: &[(&str, &str)]) -> StatusCode {
    let mut builder = Request::builder().method(Method::GET).uri(uri);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let response = app
        .clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    to_bytes(response.into_body(), usize::MAX).await.unwrap();
    status
}

/// The value of one sample line (`name{labels} value`).
fn sample(text: &str, series: &str) -> f64 {
    text.lines()
        .find_map(|line| line.strip_prefix(series)?.strip_prefix(' ')?.parse().ok())
        .unwrap_or_else(|| panic!("no sample {series} in\n{text}"))
}

#[tokio::test]
async fn requests_are_counted_by_what_answered_them() {
    let metrics = HttpMetrics::new();
    let app = app_with(&metrics).await;
    let gzip = [("accept-encoding", "gzip")];
    assert_eq!(get(&app, "/", &gzip).await, StatusCode::OK);
    assert_eq!(get(&app, "/", &[]).await, StatusCode::OK);
    assert_eq!(
        get(&app, "/_next/static/chunks/310vm2bl3xxpt.js", &gzip).await,
        StatusCode::OK
    );
    assert_eq!(
        get(&app, "/no-such-page", &gzip).await,
        StatusCode::NOT_FOUND
    );

    let text = metrics.to_prometheus();
    assert_eq!(
        sample(&text, "nextrs_http_served_total{via=\"prerendered\"}"),
        2.0
    );
    assert_eq!(
        sample(&text, "nextrs_http_served_total{via=\"static\"}"),
        1.0
    );
    assert_eq!(
        sample(&text, "nextrs_http_served_total{via=\"not_found\"}"),
        1.0
    );
    // No executor in this app: nothing reached JavaScript.
    assert_eq!(
        sample(&text, "nextrs_http_served_total{via=\"executor\"}"),
        0.0
    );
    assert_eq!(sample(&text, "nextrs_http_proxy_runs_total"), 0.0);
    // Every request was routed once, and its head time recorded by source.
    assert_eq!(sample(&text, "nextrs_http_routing_seconds_count"), 4.0);
    assert_eq!(
        sample(
            &text,
            "nextrs_http_served_head_seconds_count{via=\"prerendered\"}"
        ),
        2.0
    );
    assert_eq!(
        sample(
            &text,
            "nextrs_http_served_head_seconds_bucket{via=\"static\",le=\"+Inf\"}"
        ),
        1.0
    );
    // Labels are the fixed source names: no path ever appears.
    for line in text.lines().filter(|l| !l.starts_with('#')) {
        assert!(
            !line.contains("/_next") && !line.contains("no-such-page"),
            "{line}"
        );
    }
    let json = metrics.to_json();
    assert_eq!(json["stages"]["served"]["static"], 1);
    assert_eq!(json["stages"]["routing_seconds"]["count"], 4);
}
