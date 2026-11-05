//! Executor behaviour that does not depend on Next.js: request isolation
//! through AsyncLocalStorage, streaming, cancellation, failures and the
//! capability boundary. Handlers live in tests/fixtures/handlers/dist.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use nextrs_runtime::{Executor, ExecutorConfig, Limits, RenderRequest, RequestBody, ResponseEvent};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/handlers")
}

fn executor() -> Arc<Executor> {
    let project = fixture();
    let mut env = BTreeMap::new();
    env.insert("NODE_ENV".to_owned(), "production".to_owned());
    env.insert("NEXTRS_TEST".to_owned(), "1".to_owned());
    Executor::start(ExecutorConfig {
        dist_dir: project.join("dist"),
        project_dir: project,
        env,
        bootstrap: None,
        trace_node_api: false,
        limits: Limits::default(),
        network: None,
        cache: None,
    })
    .expect("executor starts")
}

fn request(entry: &str, headers: &[(&str, &str)]) -> RenderRequest {
    RenderRequest {
        method: "GET".into(),
        url: "/test".into(),
        headers: headers
            .iter()
            .map(|(n, v)| ((*n).to_owned(), (*v).to_owned()))
            .collect(),
        body: RequestBody::Empty,
        entry: fixture().join("dist").join(entry),
        remote_address: None,
        initial_headers: Vec::new(),
        initial_status: None,
    }
}

struct Collected {
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
    chunks: usize,
    error: Option<String>,
}

async fn collect(executor: &Executor, request: RenderRequest) -> Collected {
    let mut events = executor.render(request).await.expect("queued");
    let mut out = Collected {
        status: 0,
        headers: Vec::new(),
        body: String::new(),
        chunks: 0,
        error: None,
    };
    while let Some(event) = events.recv().await {
        match event {
            ResponseEvent::Head { status, headers } => {
                out.status = status;
                out.headers = headers;
            }
            ResponseEvent::Chunk(chunk) => {
                out.chunks += 1;
                out.body.push_str(&String::from_utf8_lossy(&chunk));
            }
            ResponseEvent::End => break,
            ResponseEvent::Error(error) => {
                out.error = Some(error);
                break;
            }
        }
    }
    out
}

const CONTEXT_CHECKS: &[&str] = &[
    "sync",
    "setTimeout",
    "setImmediate",
    "nextTick",
    "queueMicrotask",
    "timers/promises",
    "setInterval",
    "node-stream",
    "stream-event",
    "web-stream",
    "event-emitter",
    "after-rejection",
];

fn assert_own_context(body: &str, id: &str) {
    for check in CONTEXT_CHECKS {
        let expected = format!("{check}={id}");
        assert!(
            body.lines().any(|line| line == expected),
            "request {id}: missing `{expected}` in\n{body}"
        );
    }
    for line in body.lines() {
        if let Some((label, value)) = line.split_once('=') {
            match label {
                "outside-resource" | "exit" => assert_eq!(value, "undefined", "{id}: {line}"),
                "snapshot" => assert_eq!(value, id, "{id}: {line}"),
                _ => assert_eq!(value, id, "request {id} saw another context: {line}"),
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn async_local_storage_follows_a_single_request() {
    let executor = executor();
    let response = collect(&executor, request("context.js", &[("x-id", "solo")])).await;
    assert_eq!(response.status, 200, "{}", response.body);
    assert_own_context(&response.body, "solo");
}

/// Hundreds of overlapping requests in one isolate, each suspended at
/// different times. A leak shows up as another request's id.
#[tokio::test(flavor = "multi_thread")]
async fn concurrent_requests_never_observe_each_others_context() {
    let executor = executor();
    for round in 0..3 {
        let mut tasks = Vec::new();
        for i in 0..300 {
            let executor = executor.clone();
            tasks.push(tokio::spawn(async move {
                let id = format!("r{round}-{i}");
                let delay = ((i * 7919) % 23).to_string();
                let response = collect(
                    &executor,
                    request("context.js", &[("x-id", &id), ("x-delay", &delay)]),
                )
                .await;
                (id, response)
            }));
        }
        for task in tasks {
            let (id, response) = task.await.unwrap();
            assert_eq!(response.status, 200, "{id}: {}", response.body);
            assert_own_context(&response.body, &id);
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn responses_stream_chunks_with_status_and_repeated_headers() {
    let executor = executor();
    let response = collect(&executor, request("streaming.js", &[])).await;
    assert_eq!(response.status, 201);
    assert_eq!(response.body, "chunk-0\nchunk-1\nchunk-2\n");
    assert!(
        response.chunks >= 3,
        "chunks were coalesced: {}",
        response.chunks
    );
    let cookies: Vec<_> = response
        .headers
        .iter()
        .filter(|(n, _)| n.eq_ignore_ascii_case("set-cookie"))
        .map(|(_, v)| v.as_str())
        .collect();
    assert_eq!(cookies, ["a=1", "b=2"]);
}

/// Dropping the receiver is what the HTTP layer does when a client goes
/// away; the handler must observe `close` before it finished.
#[tokio::test(flavor = "multi_thread")]
async fn dropping_the_response_closes_it_in_javascript() {
    let executor = executor();
    let mut events = executor.render(request("streaming.js", &[])).await.unwrap();
    // Head, then the first chunk.
    assert!(matches!(
        events.recv().await,
        Some(ResponseEvent::Head { .. })
    ));
    assert!(matches!(events.recv().await, Some(ResponseEvent::Chunk(_))));
    drop(events);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let report = collect(&executor, request("report-closed.js", &[])).await;
    assert_eq!(report.body, "1");
}

#[tokio::test(flavor = "multi_thread")]
async fn handler_failures_are_reported_and_the_runtime_survives() {
    let executor = executor();
    let before = collect(
        &executor,
        request("errors.js", &[("x-mode", "before-head")]),
    )
    .await;
    assert_eq!(before.status, 500);
    assert!(
        before
            .headers
            .iter()
            .any(|(n, v)| n == "x-nextrs-error" && v == "handler-exception")
    );

    let after = collect(&executor, request("errors.js", &[("x-mode", "after-head")])).await;
    assert_eq!(after.status, 200);
    assert_eq!(after.body, "partial");
    assert_eq!(after.error.as_deref(), Some("boom after head"));

    let ok = collect(&executor, request("errors.js", &[])).await;
    assert_eq!(ok.body, "ok");
}

#[tokio::test(flavor = "multi_thread")]
async fn unsupported_apis_fail_explicitly() {
    let executor = executor();
    let child = collect(
        &executor,
        request("errors.js", &[("x-mode", "unsupported")]),
    )
    .await;
    assert!(
        child.body.starts_with(
            "ERR_NEXTRS_UNSUPPORTED|[NextRS] node:child_process.spawn is not supported"
        ),
        "{}",
        child.body
    );
    assert!(
        child.body.contains("errors.js"),
        "caller missing: {}",
        child.body
    );
    // Without a network configuration, fetch is denied by policy, as a
    // Node-style network error (TypeError with the reason as its cause).
    let fetch = collect(&executor, request("errors.js", &[("x-mode", "fetch")])).await;
    assert!(
        fetch
            .body
            .starts_with("ERR_NEXTRS_NET_DENIED|TypeError|fetch failed|"),
        "{}",
        fetch.body
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn application_code_only_reaches_granted_capabilities() {
    let executor = executor();
    let response = collect(&executor, request("capabilities.js", &[])).await;
    let results: BTreeMap<&str, &str> = response
        .body
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect();
    let expect = |name: &str, value: &str| {
        assert_eq!(
            results.get(name).copied(),
            Some(value),
            "{name}\n{}",
            response.body
        );
    };
    expect("read-own", "ok:true");
    expect("read-traversal", "EACCES");
    expect("read-encoded-traversal", "ENOENT");
    expect("read-etc-passwd", "EACCES");
    expect("exists-outside", "ok:false");
    expect("stat-missing-outside", "EACCES");
    expect("require-outside", "MODULE_NOT_FOUND");
    expect("write", "EROFS");
    expect("env-keys", "ok:NEXTRS_TEST,NODE_ENV");
    expect("deno-global", "ok:undefined");
    expect("internal-global", "ok:undefined");
    expect("process-exit", "ERR_NEXTRS_UNSUPPORTED");
    expect("vm-context", "ok:42,undefined,undefined");
    // Reference values computed with Node 22 (first 40 characters).
    expect(
        "crypto-sha256",
        "ok:f5cab92ac3a33a5217dab3bf23991a3de3c5b607",
    );
    expect("crypto-hmac", "ok:InKOg_ANwCSyMVWw_TaA913nV49-eikir4WnCmri");
}
