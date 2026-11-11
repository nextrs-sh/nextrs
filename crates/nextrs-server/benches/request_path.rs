//! Rust request-path microbenchmarks: the whole server (routing, file and
//! prerendered responses, headers) driven in process against a redacted
//! Next.js 16.4.3 build snapshot, plus the routing primitives every request
//! uses. No sockets, no JavaScript.
//!
//!   cargo bench -p nextrs-server --bench request_path
//!
//! Wall-clock results depend on the machine: compare runs on the same host
//! (`--save-baseline` / `--baseline`), never against fixed thresholds.

use std::hint::black_box;
use std::path::Path;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Request, header};
use criterion::{Criterion, criterion_group, criterion_main};
use nextrs_server::ServerOptions;
use nextrs_server::routing::{query, url};
use tower::ServiceExt;

async fn app() -> Router {
    let snapshot =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/data/next-16.4.3-default");
    let info = nextrs_adapter::inspect(&snapshot).unwrap();
    nextrs_server::app(&info, &ServerOptions::default())
        .await
        .unwrap()
}

async fn send(app: &Router, request: Request<Body>) -> usize {
    let response = app.clone().oneshot(request).await.unwrap();
    to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap()
        .len()
}

fn get(uri: &str) -> Request<Body> {
    Request::get(uri)
        .header(header::ACCEPT, "text/html,application/xhtml+xml")
        .header(header::USER_AGENT, "bench")
        .header(header::ACCEPT_ENCODING, "identity")
        .body(Body::empty())
        .unwrap()
}

fn requests(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let app = rt.block_on(app());
    let etag = rt.block_on(async {
        let response = app.clone().oneshot(get("/")).await.unwrap();
        response.headers()[header::ETAG]
            .to_str()
            .unwrap()
            .to_owned()
    });
    let mut group = c.benchmark_group("request");
    for (name, uri) in [
        ("prerendered_page", "/"),
        (
            "static_chunk",
            "/_next/static/chunks/turbopack-3-ry682tkyiqk.js",
        ),
        ("static_missing", "/_next/static/chunks/missing.js"),
        ("trailing_slash_redirect", "/books/42/?x=1"),
        ("query_string", "/?utm_source=a&utm_medium=b&x=1&x=2"),
    ] {
        group.bench_function(name, |b| b.iter(|| rt.block_on(send(&app, get(uri)))));
    }
    group.bench_function("conditional_304", |b| {
        b.iter(|| {
            let mut request = get("/");
            request
                .headers_mut()
                .insert(header::IF_NONE_MATCH, etag.parse().unwrap());
            rt.block_on(send(&app, request))
        })
    });
    group.finish();
}

fn routing(c: &mut Criterion) {
    let mut group = c.benchmark_group("routing");
    for (name, input) in [
        (
            "parse_url_path",
            "/_next/static/chunks/turbopack-3-ry682tkyiqk.js",
        ),
        (
            "parse_url_query",
            "/books/42?ref=bench&utm_source=x&a=1&a=2",
        ),
        ("parse_url_needs_parser", "/a/./b/../c%2e?x=%7B1%7D#frag"),
    ] {
        group.bench_function(name, |b| {
            b.iter(|| url::parse_relative_url(black_box(input)))
        });
    }
    group.bench_function("parse_search", |b| {
        b.iter(|| {
            query::parse_search(black_box(
                "utm_source=a&utm_medium=b&x=1&x=2&q=hello%20world",
            ))
        })
    });
    group.finish();
}

criterion_group!(benches, requests, routing);
criterion_main!(benches);
