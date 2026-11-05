//! Embedded V8 executor for official Next.js route handlers.
//!
//! An [`Executor`] is a pool of one or more V8 isolates. Each isolate runs on
//! its own thread with its own current-thread Tokio runtime, loads Next's
//! adapter bootstrap (`next/dist/build/adapter/setup-node-env.external.js`),
//! and then calls each compiled route's `handler(req, res, ctx)`, the same
//! entry point Next's own server uses. Requests dispatched to one isolate run
//! concurrently in it; request context is kept apart by `AsyncLocalStorage`,
//! implemented on V8's continuation-preserved embedder data.
//!
//! Every isolate is supervised (`pool.rs`): a failed isolate stops receiving
//! work, its in-flight requests fail, and a fresh isolate replaces it with
//! backoff. Admission is bounded (per-isolate concurrency plus a bounded,
//! deadline-limited queue), responses are flow-controlled against a byte
//! budget (`response.rs`), and request bodies are pulled on demand.
//!
//! The JavaScript side gets Web APIs from `deno_web`, Node built-ins from
//! `js/` (NextRS-owned) and `js/node-polyfills.js` (vendored npm packages),
//! and host capabilities only through the ops in `ops.rs`.

mod cache;
mod metrics;
mod net;
mod ops;
mod pool;
mod response;
mod sandbox;
mod worker;

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::pin::Pin;
use std::time::Duration;

use bytes::Bytes;

pub use crate::cache::CacheConfig;
pub use crate::metrics::{CacheSnapshot, ExecutorSnapshot, FetchSnapshot, MetricsSnapshot};
pub use crate::net::{AddrRange, AllowRule, Denied, NetworkConfig, NetworkLimits, NetworkPolicy};
pub use crate::pool::{Executor, FailureReason};
pub use crate::response::{ResponseEvent, ResponseStream};
pub use crate::sandbox::{FsDenied, FsPolicy};

/// Passes flags to V8 (e.g. `--max-semi-space-size=8`). Process-wide; call
/// before the first [`Executor`] starts. Returns the flags V8 did not
/// recognize. For tuning experiments: some V8 flags weaken the sandbox
/// (`--allow-natives-syntax` exposes engine internals to JavaScript).
pub fn set_v8_flags(flags: &[String]) -> Vec<String> {
    let mut args = vec!["nextrs".to_owned()];
    args.extend(flags.iter().cloned());
    deno_core::v8_set_flags(args).into_iter().skip(1).collect()
}

/// The Node.js version the runtime reports in `process.version`: the
/// version NextRS is compatibility-tested against, not a claim to be Node.
pub const REPORTED_NODE_VERSION: &str = "22.20.0";

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
    pub limits: Limits,
    /// Outbound `fetch`. `None`: JavaScript has no network access, and
    /// `fetch` fails with `ERR_NEXTRS_NET_DENIED`.
    pub network: Option<NetworkConfig>,
    /// The data cache shared by every isolate (fetch cache, unstable_cache),
    /// offered to Next.js as its cache handler. `None`: Next's default
    /// handler, separate in each isolate and in memory only (the build
    /// directory is read-only).
    pub cache: Option<CacheConfig>,
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
            limits: Limits::default(),
            network: None,
            cache: Some(CacheConfig::default()),
        }
    }

    /// Read access: the build output and the installed packages. The
    /// project root itself (`.env*`, sources, `next.config.*`) is excluded.
    pub fn fs_policy(&self) -> FsPolicy {
        FsPolicy::new([self.dist_dir.clone(), self.project_dir.join("node_modules")])
    }
}

/// Resource limits of an [`Executor`]. What each one can and cannot
/// guarantee is described on the field; in short, every limit is enforced
/// per request except CPU and heap, which V8 can only enforce per isolate.
#[derive(Debug, Clone)]
pub struct Limits {
    /// V8 isolates (each with its own thread, heap and event loop).
    pub isolates: usize,
    /// Requests executing concurrently inside one isolate.
    pub max_active_per_isolate: usize,
    /// Requests waiting for a slot when every isolate is at its limit.
    /// Beyond this, requests are refused at once ([`RenderError::Overloaded`]).
    pub max_queued: usize,
    /// How long a request may wait for a slot ([`RenderError::QueueTimeout`]).
    pub queue_timeout: Duration,
    /// Deadline from delivery to the isolate until the response has ended
    /// and the request's `waitUntil`/`after()` work has settled. On expiry
    /// the request is cancelled: 504 if no response has started, otherwise
    /// the response is aborted; `req`/`res` emit `close`, and pending
    /// `waitUntil` work is abandoned. It does not stop JavaScript that is
    /// busy on the CPU: that is `stall_timeout`. `None` disables it.
    pub request_timeout: Option<Duration>,
    /// Longest time an isolate's thread may run JavaScript without
    /// returning to its event loop. Beyond it the **whole isolate** is
    /// terminated (`TerminateExecution`) and replaced: every request in
    /// it fails, not only the one that looped. `None` disables it.
    pub stall_timeout: Option<Duration>,
    /// V8 heap limit per isolate. Reaching it terminates and replaces the
    /// isolate instead of aborting the process. `None` keeps V8's default
    /// limit (the near-limit handler is installed either way).
    pub heap_limit_bytes: Option<usize>,
    /// Response body bytes buffered per response between JavaScript and
    /// the socket before `res.write()` applies backpressure.
    pub response_buffer_bytes: usize,
    /// Restart backoff after an isolate fails: doubles with each failure
    /// within a minute, up to `restart_backoff_max`.
    pub restart_backoff_initial: Duration,
    pub restart_backoff_max: Duration,
    /// On shutdown, how long in-flight requests may run before their
    /// isolates are terminated.
    pub shutdown_grace: Duration,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            isolates: 1,
            max_active_per_isolate: 128,
            max_queued: 512,
            queue_timeout: Duration::from_secs(10),
            request_timeout: Some(Duration::from_secs(60)),
            stall_timeout: Some(Duration::from_secs(10)),
            heap_limit_bytes: None,
            response_buffer_bytes: 64 * 1024,
            restart_backoff_initial: Duration::from_millis(100),
            restart_backoff_max: Duration::from_secs(30),
            shutdown_grace: Duration::from_secs(10),
        }
    }
}

/// One HTTP request for a route handler.
#[derive(Debug)]
pub struct RenderRequest {
    pub method: String,
    /// Path and query, as received.
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: RequestBody,
    /// Absolute path of the compiled route entry (`.next/server/app/**/page.js`).
    pub entry: PathBuf,
    pub remote_address: Option<String>,
    /// Headers the serving layer puts on the response before the handler
    /// runs (next start's base server does this for `Vary`). The handler
    /// may override them.
    pub initial_headers: Vec<(String, String)>,
}

/// A request body stream. JavaScript pulls one item per `_read()`, so the
/// producer (the HTTP connection) only reads as fast as the handler
/// consumes. Size limits are the producer's job: it should yield
/// [`BodyError::TooLarge`] once the limit is crossed.
pub type BodyStream = Pin<Box<dyn futures_util::Stream<Item = Result<Bytes, BodyError>> + Send>>;

pub enum RequestBody {
    Empty,
    Full(Bytes),
    Stream(BodyStream),
}

impl fmt::Debug for RequestBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("Empty"),
            Self::Full(bytes) => write!(f, "Full({} bytes)", bytes.len()),
            Self::Stream(_) => f.write_str("Stream"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BodyError {
    /// The body exceeded the configured limit.
    TooLarge { limit: u64 },
    /// The connection failed or the client aborted the upload.
    Aborted(String),
}

impl fmt::Display for BodyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { limit } => write!(f, "request body exceeds the {limit}-byte limit"),
            Self::Aborted(reason) => write!(f, "request body aborted: {reason}"),
        }
    }
}

#[derive(Debug)]
pub enum ExecutorError {
    /// Bootstrapping JavaScript failed (with the JS error and stack).
    Bootstrap(String),
    /// The executor has stopped.
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

/// Why a request was not handed to an isolate. JavaScript never saw it, so
/// a client may retry it whatever its method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderError {
    /// Every isolate is at its concurrency limit and the queue is full.
    Overloaded,
    /// The request waited `queue_timeout` without getting a slot.
    QueueTimeout,
    /// The executor is shutting down.
    ShuttingDown,
    /// The chosen isolate failed before the request could be delivered.
    Unavailable,
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Overloaded => "every JavaScript isolate is busy and the queue is full",
            Self::QueueTimeout => "timed out waiting for a JavaScript isolate",
            Self::ShuttingDown => "the JavaScript executor is shutting down",
            Self::Unavailable => "the JavaScript isolate became unavailable",
        })
    }
}

impl std::error::Error for RenderError {}

impl RenderError {
    /// Stable machine-readable name (used in `x-nextrs-error`).
    pub fn code(self) -> &'static str {
        match self {
            Self::Overloaded => "executor-overloaded",
            Self::QueueTimeout => "executor-queue-timeout",
            Self::ShuttingDown => "executor-shutting-down",
            Self::Unavailable => "executor-unavailable",
        }
    }
}
