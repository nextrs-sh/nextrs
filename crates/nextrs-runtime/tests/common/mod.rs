//! Helpers shared by the executor integration tests. Handlers live in
//! tests/fixtures/handlers/dist; no Next.js build is needed.
#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use nextrs_runtime::{
    Executor, ExecutorConfig, Limits, RenderError, RenderRequest, RequestBody, ResponseEvent,
    ResponseStream,
};

pub fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/handlers")
}

pub fn config(limits: Limits) -> ExecutorConfig {
    let project = fixture();
    let mut env = BTreeMap::new();
    env.insert("NODE_ENV".to_owned(), "production".to_owned());
    env.insert("NEXTRS_TEST".to_owned(), "1".to_owned());
    ExecutorConfig {
        dist_dir: project.join("dist"),
        project_dir: project,
        workspace_dir: None,
        env,
        bootstrap: None,
        trace_node_api: false,
        limits,
        network: None,
        cache: None,
        build_route_cache: Vec::new(),
    }
}

pub fn executor(limits: Limits) -> Arc<Executor> {
    Executor::start(config(limits)).expect("executor starts")
}

pub fn request(entry: &str, headers: &[(&str, &str)]) -> RenderRequest {
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
        init_url: None,
        invocation: nextrs_runtime::Invocation::Handler,
    }
}

#[derive(Debug, Default)]
pub struct Collected {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub chunks: usize,
    pub error: Option<String>,
    pub ended: bool,
    /// JavaScript received the request (read after the stream ended).
    pub delivered: bool,
}

impl Collected {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Reads a response to its end. `ended` is false when the stream closed
/// without `End` or `Error` (the isolate went away).
pub async fn drain(mut events: ResponseStream) -> Collected {
    let mut out = Collected::default();
    while let Some(event) = events.recv().await {
        match event {
            ResponseEvent::Head { status, headers } => {
                out.status = status;
                out.headers = headers;
            }
            ResponseEvent::Chunk(chunk) => {
                out.chunks += 1;
                out.body.extend_from_slice(&chunk);
            }
            ResponseEvent::End => {
                out.ended = true;
                break;
            }
            ResponseEvent::Error(error) => {
                out.error = Some(error);
                break;
            }
        }
    }
    out.delivered = events.was_delivered();
    out
}

pub async fn collect(executor: &Executor, request: RenderRequest) -> Collected {
    drain(executor.render(request).await.expect("admitted")).await
}

pub async fn try_collect(
    executor: &Executor,
    request: RenderRequest,
) -> Result<Collected, RenderError> {
    Ok(drain(executor.render(request).await?).await)
}

/// Polls `check` until it holds or `timeout` passes.
pub async fn eventually(timeout: Duration, mut check: impl AsyncFnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if check().await {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    check().await
}

/// Requests until one is answered by a ready isolate.
pub async fn whoami(executor: &Executor) -> String {
    collect(executor, request("faults.js", &[("x-mode", "whoami")]))
        .await
        .text()
}

pub fn small_limits() -> Limits {
    Limits {
        stall_timeout: Some(Duration::from_millis(500)),
        restart_backoff_initial: Duration::from_millis(20),
        restart_backoff_max: Duration::from_millis(200),
        shutdown_grace: Duration::from_secs(2),
        ..Limits::default()
    }
}

/// Set in a child process started by [`run_in_child`].
const CHILD: &str = "NEXTRS_TEST_CHILD";

/// Whether this process is a child started by [`run_in_child`] for `case`.
/// Child-only tests return at once otherwise, so a normal run (including
/// `--ignored`) never executes a case that may abort the process.
#[allow(dead_code)]
pub fn is_child(case: &str) -> bool {
    std::env::var(CHILD).is_ok_and(|v| v == case)
}

/// Runs the test `test` of this test binary alone, in a child process,
/// with `case` and `env` set: for faults that end the whole process (a
/// fatal V8 out-of-memory abort), which must not take the test runner
/// with them. Returns the child's output and exit status.
#[allow(dead_code)]
pub fn run_in_child(test: &str, case: &str, env: &[(&str, &str)]) -> std::process::Output {
    let exe = std::env::current_exe().expect("test binary path");
    let mut command = std::process::Command::new(exe);
    command
        .args([test, "--exact", "--nocapture", "--test-threads=1"])
        .env(CHILD, case);
    for (name, value) in env {
        command.env(name, value);
    }
    command.output().expect("child test process")
}
