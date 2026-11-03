//! Embedded V8 executor for official Next.js route handlers.
//!
//! One V8 isolate runs on a dedicated thread with its own current-thread
//! Tokio runtime. It loads Next's adapter bootstrap
//! (`next/dist/build/adapter/setup-node-env.external.js`) and then calls each
//! compiled route's `handler(req, res, ctx)`, the same entry point Next's own
//! server uses. Requests are dispatched concurrently into that one isolate;
//! request context is kept apart by `AsyncLocalStorage`, implemented on V8's
//! continuation-preserved embedder data.
//!
//! The JavaScript side gets Web APIs from `deno_web`, Node built-ins from
//! `js/` (NextRS-owned) and `js/node-polyfills.js` (vendored npm packages),
//! and host capabilities only through the ops in `ops.rs`.

mod ops;
mod sandbox;

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::thread::JoinHandle;

use deno_core::{JsRuntime, PollEventLoopOptions, RuntimeOptions};
use tokio::sync::mpsc;

pub use crate::sandbox::{FsDenied, FsPolicy};

/// The Node.js version the runtime reports in `process.version`: the
/// version NextRS is compatibility-tested against, not a claim to be Node.
pub const REPORTED_NODE_VERSION: &str = "22.20.0";

const SCRIPTS: &[(&str, &str)] = &[
    (
        "ext:nextrs/00_globals.js",
        include_str!("../js/00_globals.js"),
    ),
    ("ext:nextrs/01_fetch.js", include_str!("../js/01_fetch.js")),
    (
        "ext:nextrs/node-polyfills.js",
        include_str!("../js/node-polyfills.js"),
    ),
    ("ext:nextrs/02_node.js", include_str!("../js/02_node.js")),
    (
        "ext:nextrs/03_server.js",
        include_str!("../js/03_server.js"),
    ),
];

#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    /// The Next.js project directory (contains `node_modules`).
    pub project_dir: PathBuf,
    /// Its build output, normally `<project>/.next`.
    pub dist_dir: PathBuf,
    /// The complete `process.env` seen by JavaScript. Nothing from the host
    /// environment is inherited implicitly.
    pub env: BTreeMap<String, String>,
    /// Module required once before any request: Next's official adapter
    /// bootstrap (`setup-node-env.external.js`) by default.
    pub bootstrap: Option<PathBuf>,
    /// Report every Node built-in module and member JavaScript touches
    /// (once each, as `node-api-access` diagnostics). For compatibility
    /// research; off by default.
    pub trace_node_api: bool,
}

impl ExecutorConfig {
    pub fn new(project_dir: PathBuf, dist_dir: PathBuf) -> Self {
        let mut env = BTreeMap::new();
        env.insert("NODE_ENV".into(), "production".into());
        let bootstrap =
            project_dir.join("node_modules/next/dist/build/adapter/setup-node-env.external.js");
        Self {
            project_dir,
            dist_dir,
            env,
            bootstrap: Some(bootstrap),
            trace_node_api: false,
        }
    }

    /// Read access: the build output and the installed packages. The
    /// project root itself (`.env*`, sources, `next.config.*`) is excluded.
    pub fn fs_policy(&self) -> FsPolicy {
        FsPolicy::new([self.dist_dir.clone(), self.project_dir.join("node_modules")])
    }
}

/// One HTTP request for a route handler.
#[derive(Debug, Clone)]
pub struct RenderRequest {
    pub method: String,
    /// Path and query, as received.
    pub url: String,
    pub headers: Vec<(String, String)>,
    /// Buffered request body. Streaming request bodies are not supported.
    pub body: Vec<u8>,
    /// Absolute path of the compiled route entry (`.next/server/app/**/page.js`).
    pub entry: PathBuf,
    pub remote_address: Option<String>,
    /// Headers the serving layer puts on the response before the handler
    /// runs (next start's base server does this for `Vary`). The handler
    /// may override them.
    pub initial_headers: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseEvent {
    Head {
        status: u16,
        headers: Vec<(String, String)>,
    },
    Chunk(Vec<u8>),
    End,
    /// The handler failed after the response started, or the executor
    /// failed. The body is incomplete.
    Error(String),
}

pub(crate) struct Job {
    request: RenderRequest,
    events: mpsc::UnboundedSender<ResponseEvent>,
}

#[derive(Debug)]
pub enum ExecutorError {
    /// Bootstrapping JavaScript failed (with the JS error and stack).
    Bootstrap(String),
    /// The executor thread has stopped.
    Stopped,
    Io(std::io::Error),
}

impl fmt::Display for ExecutorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bootstrap(error) => write!(f, "JavaScript bootstrap failed: {error}"),
            Self::Stopped => f.write_str("the JavaScript executor has stopped"),
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ExecutorError {}

/// Handle to the executor thread. Dropping it shuts the isolate down after
/// in-flight requests finish.
pub struct Executor {
    jobs: Option<mpsc::UnboundedSender<Job>>,
    thread: Option<JoinHandle<()>>,
}

impl Executor {
    /// Starts the isolate and loads Next's Node bootstrap. Returns once
    /// JavaScript is ready to accept requests.
    pub fn start(config: ExecutorConfig) -> Result<Arc<Self>, ExecutorError> {
        let (jobs, inbox) = mpsc::unbounded_channel();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let thread = std::thread::Builder::new()
            .name("nextrs-v8".into())
            .spawn(move || run_isolate(config, inbox, ready_tx))
            .map_err(ExecutorError::Io)?;
        match ready_rx.recv() {
            Ok(Ok(())) => Ok(Arc::new(Self {
                jobs: Some(jobs),
                thread: Some(thread),
            })),
            Ok(Err(error)) => {
                drop(jobs);
                let _ = thread.join();
                Err(ExecutorError::Bootstrap(error))
            }
            Err(_) => {
                let _ = thread.join();
                Err(ExecutorError::Stopped)
            }
        }
    }

    /// Queues a request. Events arrive on the returned channel: one `Head`,
    /// any number of `Chunk`s, then `End` or `Error`. Dropping the receiver
    /// signals cancellation to JavaScript (`res` emits `close`).
    pub fn render(
        &self,
        request: RenderRequest,
    ) -> Result<mpsc::UnboundedReceiver<ResponseEvent>, ExecutorError> {
        let (events, receiver) = mpsc::unbounded_channel();
        self.jobs
            .as_ref()
            .ok_or(ExecutorError::Stopped)?
            .send(Job { request, events })
            .map_err(|_| ExecutorError::Stopped)?;
        Ok(receiver)
    }
}

impl fmt::Debug for Executor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Executor")
            .field("running", &self.jobs.is_some())
            .finish()
    }
}

impl Drop for Executor {
    fn drop(&mut self) {
        self.jobs.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run_isolate(
    config: ExecutorConfig,
    inbox: mpsc::UnboundedReceiver<Job>,
    ready: std::sync::mpsc::Sender<Result<(), String>>,
) {
    let tokio = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            let _ = ready.send(Err(error.to_string()));
            return;
        }
    };
    let failure = ready.clone();
    let result = tokio.block_on(async move {
        let boot = ops::BootConfig {
            project_dir: config.project_dir.to_string_lossy().into_owned(),
            dist_dir: config.dist_dir.to_string_lossy().into_owned(),
            env: config.env.clone(),
            bootstrap: config
                .bootstrap
                .as_ref()
                .map(|path| path.to_string_lossy().into_owned()),
            trace_node_api: config.trace_node_api,
            node_version: REPORTED_NODE_VERSION,
            nextrs_version: env!("CARGO_PKG_VERSION"),
        };
        let mut runtime = JsRuntime::new(RuntimeOptions {
            extensions: vec![
                deno_webidl::deno_webidl::init(),
                deno_web::deno_web::init(
                    Arc::new(deno_web::BlobStore::default()),
                    None,
                    false,
                    deno_web::InMemoryBroadcastChannel::default(),
                ),
                ops::nextrs_runtime::init(
                    boot,
                    config.fs_policy(),
                    ops::Inbox(Rc::new(tokio::sync::Mutex::new(inbox))),
                    ops::ReadySignal(Some(ready)),
                ),
            ],
            ..Default::default()
        });
        for (name, source) in SCRIPTS {
            runtime
                .execute_script(*name, *source)
                .map_err(|e| format!("{e}"))?;
        }
        runtime
            .execute_script("ext:nextrs/start", "globalThis.__nextrsStart()")
            .map_err(|e| format!("{e}"))?;
        runtime
            .run_event_loop(PollEventLoopOptions::default())
            .await
            .map_err(|e| format!("{e}"))
    });
    if let Err(error) = result {
        tracing::error!(target: "nextrs::runtime", "JavaScript executor stopped: {error}");
        // Only reaches start() if bootstrap had not reported yet.
        let _ = failure.send(Err(error));
    }
}
