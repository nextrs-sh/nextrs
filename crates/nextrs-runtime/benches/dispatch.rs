//! Executor microbenchmarks: one request through admission, dispatch to the
//! isolate, the handler, and the response back to Rust (no HTTP). Measures
//! the Rust–V8 boundary and NextRS's JavaScript layer for small responses.
//!
//!   cargo bench -p nextrs-runtime --bench dispatch
//!
//! Wall-clock results depend on the machine: compare runs on the same host
//! (`--save-baseline` / `--baseline`), never against fixed thresholds.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use criterion::{Criterion, criterion_group, criterion_main};
use nextrs_runtime::{
    Executor, ExecutorConfig, Invocation, Limits, RenderRequest, RequestBody, ResponseEvent,
};

fn handlers() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/handlers")
}

fn executor() -> Arc<Executor> {
    let project = handlers();
    let mut env = BTreeMap::new();
    env.insert("NODE_ENV".to_owned(), "production".to_owned());
    Executor::start(ExecutorConfig {
        dist_dir: project.join("dist"),
        project_dir: project,
        env,
        bootstrap: None,
        trace_node_api: false,
        limits: Limits::default(),
        network: None,
        cache: None,
        build_route_cache: Vec::new(),
    })
    .expect("executor starts")
}

fn request(entry: &str, headers: &[(&str, &str)]) -> RenderRequest {
    RenderRequest {
        method: "GET".into(),
        url: "/bench?x=1".into(),
        headers: headers
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect(),
        body: RequestBody::Empty,
        entry: handlers().join("dist").join(entry),
        remote_address: None,
        initial_headers: Vec::new(),
        initial_status: None,
        init_url: None,
        invocation: Invocation::Handler,
    }
}

/// Renders and drains one response; returns its body length.
async fn render(executor: &Executor, request: RenderRequest) -> usize {
    let mut events = executor.render(request).await.expect("admitted");
    let mut bytes = 0;
    while let Some(event) = events.recv().await {
        match event {
            ResponseEvent::Chunk(chunk) => bytes += chunk.len(),
            ResponseEvent::End => break,
            ResponseEvent::Error(error) => panic!("{error}"),
            ResponseEvent::Head { .. } => {}
        }
    }
    bytes
}

fn dispatch(c: &mut Criterion) {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let executor = rt.block_on(async { executor() });
    let headers = [
        ("accept", "text/html"),
        ("user-agent", "bench"),
        ("cookie", "a=1; b=2"),
        ("x-id", "42"),
    ];
    let mut group = c.benchmark_group("dispatch");
    group.bench_function("empty_response", |b| {
        b.iter(|| {
            rt.block_on(render(
                &executor,
                request("echo.js", &[("x-skip-body", "1")]),
            ))
        })
    });
    group.bench_function("html_16k_in_8_writes", |b| {
        b.iter(|| rt.block_on(render(&executor, request("bench-html.js", &headers))))
    });
    group.bench_function("html_64k_in_32_writes", |b| {
        let mut h = headers.to_vec();
        h.push(("x-chunks", "32"));
        b.iter(|| rt.block_on(render(&executor, request("bench-html.js", &h))))
    });
    group.finish();
}

criterion_group!(benches, dispatch);
criterion_main!(benches);
