//! The only capabilities JavaScript gets from the host. Everything a Next.js
//! handler can do outside the V8 heap goes through one of these ops.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Instant, UNIX_EPOCH};

use bytes::Bytes;
use deno_core::{OpState, ToJsBuffer, op2, v8};
use deno_error::JsErrorBox;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

use crate::cache::{CacheStore, EntryKind, Hit, LeaseOutcome, Lookup, SetOptions, TagSync};
use crate::metrics::{self, Counters};
use crate::net::{BodyItem, FetchError, FetchHead, FetchRequest, NetClient, OutBody, RedirectMode};
use crate::pool::{Job, Slot, WorkerShared};
use crate::response::{FlowHandle, ResponseEvent, ResponseSender, WriteStatus};
use crate::sandbox::{FsDenied, FsPolicy};
use crate::{BodyError, BodyStream, RequestBody};

/// Static configuration handed to bootstrap.js.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BootConfig {
    pub project_dir: String,
    pub dist_dir: String,
    pub env: BTreeMap<String, String>,
    pub bootstrap: Option<String>,
    pub trace_node_api: bool,
    pub node_version: &'static str,
    pub nextrs_version: &'static str,
    pub fault_injection: bool,
    /// Outbound fetch is configured (the policy itself stays in Rust).
    pub network: bool,
    /// The shared data cache is offered to Next.js as its cache handler.
    pub cache: bool,
}

pub(crate) struct Inbox(pub Rc<tokio::sync::Mutex<mpsc::UnboundedReceiver<Box<Job>>>>);

/// A request JavaScript is working on. Dropping it (`op_nextrs_request_done`,
/// or the whole op state when the isolate goes away) releases the slot.
struct Open {
    /// Taken when the response ends.
    response: Option<ResponseSender>,
    flow: FlowHandle,
    /// Taken while a read is in progress, and at end of stream.
    body: Option<BodyStream>,
    _slot: Slot,
    delivered_at: Instant,
}

#[derive(Default)]
pub(crate) struct Requests {
    next_id: u32,
    open: HashMap<u32, Open>,
}

pub(crate) struct ReadySignal(pub Option<std::sync::mpsc::Sender<Result<(), String>>>);

/// Host services shared by every isolate of a pool.
#[derive(Clone)]
pub(crate) struct HostServices {
    pub counters: Arc<Counters>,
    pub net: Option<Arc<NetClient>>,
    pub cache: Option<Arc<CacheStore>>,
    pub internal: crate::internal::InternalRoute,
}

/// The shared outbound HTTP client, if the network is configured.
pub(crate) struct NetCapability(pub Option<Arc<NetClient>>);

/// The data cache shared by all isolates, if enabled.
pub(crate) struct CacheCapability(pub Option<Arc<CacheStore>>);

/// Outbound fetches an isolate holds open. Dropping one (`op_nextrs_fetch_close`,
/// or the whole op state when the isolate goes away) aborts its network task.
#[derive(Default)]
pub(crate) struct Fetches {
    next_id: u32,
    open: HashMap<u32, OpenFetch>,
}

struct OpenFetch {
    head: Option<oneshot::Receiver<Result<FetchHead, FetchError>>>,
    body: Option<mpsc::Receiver<BodyItem>>,
    upload: Option<mpsc::Sender<Bytes>>,
    abort: tokio::task::AbortHandle,
    /// The response ended (body read to the end, or failed).
    settled: bool,
    counters: Arc<Counters>,
}

impl Drop for OpenFetch {
    fn drop(&mut self) {
        self.abort.abort();
        if !self.settled {
            self.counters.fetch_failed(metrics::FETCH_ABORTED);
        }
        self.counters
            .fetch_in_flight
            .fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
    }
}

deno_core::extension!(
    nextrs_runtime,
    deps = [deno_webidl, deno_web],
    ops = [
        op_nextrs_boot_config,
        op_nextrs_ready,
        op_nextrs_diagnostic,
        op_nextrs_count,
        op_nextrs_next_request,
        op_nextrs_request_read,
        op_nextrs_request_done,
        op_nextrs_response_head,
        op_nextrs_response_write,
        op_nextrs_response_drain,
        op_nextrs_response_end,
        op_nextrs_response_error,
        op_nextrs_response_closed,
        op_nextrs_fault,
        op_nextrs_fs_read,
        op_nextrs_fs_stat,
        op_nextrs_fs_readdir,
        op_nextrs_fs_realpath,
        op_nextrs_hash,
        op_nextrs_random_fill,
        op_nextrs_vm_run_in_new_context,
        op_nextrs_fetch_start,
        op_nextrs_fetch_head,
        op_nextrs_fetch_read,
        op_nextrs_fetch_write,
        op_nextrs_fetch_write_end,
        op_nextrs_fetch_close,
        op_nextrs_cache_get,
        op_nextrs_cache_set,
        op_nextrs_cache_revalidate,
        op_nextrs_cache_tag_sync,
        op_nextrs_cache_lease,
        op_nextrs_cache_release,
        crate::webcrypto::op_nextrs_aes_import,
        crate::webcrypto::op_nextrs_aes_generate,
        crate::webcrypto::op_nextrs_aes_export,
        crate::webcrypto::op_nextrs_aes_release,
        crate::webcrypto::op_nextrs_aes_gcm,
        crate::multipart::op_nextrs_multipart_parse,
    ],
    options = {
        boot: BootConfig,
        policy: FsPolicy,
        inbox: Inbox,
        ready: ReadySignal,
        worker: Arc<WorkerShared>,
        services: HostServices,
    },
    state = |state, options| {
        state.put(options.boot);
        state.put(options.policy);
        state.put(options.inbox);
        state.put(options.ready);
        state.put(options.worker);
        state.put(options.services.counters);
        state.put(Requests::default());
        state.put(NetCapability(options.services.net));
        state.put(Fetches::default());
        state.put(CacheCapability(options.services.cache));
        state.put(crate::webcrypto::CryptoKeys::default());
        state.put(options.services.internal);
    }
);

#[op2]
#[serde]
fn op_nextrs_boot_config(state: &mut OpState) -> BootConfig {
    state.borrow::<BootConfig>().clone()
}

#[op2]
fn op_nextrs_ready(state: &mut OpState, #[string] error: Option<String>) {
    if let Some(sender) = state.borrow_mut::<ReadySignal>().0.take() {
        let _ = sender.send(error.map_or(Ok(()), Err));
    }
}

/// Unsupported APIs, missing built-in members and handler failures,
/// reported by the bootstrap scripts. Never request headers or bodies.
#[op2(fast)]
fn op_nextrs_diagnostic(#[string] kind: &str, #[string] detail: &str) {
    tracing::warn!(target: "nextrs::js", kind, "{detail}");
}

/// Request outcomes only JavaScript observes (`metrics::COUNT_*`).
#[op2(fast)]
fn op_nextrs_count(state: &mut OpState, #[smi] kind: u32) {
    state.borrow::<Arc<Counters>>().count(kind);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsRequest {
    id: u32,
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    /// A body that arrived complete.
    body: Option<ToJsBuffer>,
    /// The body is pulled with `op_nextrs_request_read`.
    body_stream: bool,
    entry: String,
    remote_address: Option<String>,
    initial_headers: Vec<(String, String)>,
    initial_status: Option<u16>,
    timeout_ms: Option<u64>,
    invocation: JsInvocation,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsInvocation {
    /// `handler`, `routed` or `proxy`.
    kind: &'static str,
    url: Option<String>,
    middleware_cookies: Vec<String>,
    page: Option<String>,
    invoke_path: Option<String>,
    query: Option<String>,
}

impl From<crate::Invocation> for JsInvocation {
    fn from(invocation: crate::Invocation) -> Self {
        let mut js = Self {
            kind: "handler",
            url: None,
            middleware_cookies: Vec::new(),
            page: None,
            invoke_path: None,
            query: None,
        };
        match invocation {
            crate::Invocation::Handler => {}
            crate::Invocation::Proxy { url } => {
                js.kind = "proxy";
                js.url = Some(url);
            }
            crate::Invocation::Routed(meta) => {
                js.kind = "routed";
                js.middleware_cookies = meta.middleware_cookies;
                if let Some(rewrite) = meta.rewrite {
                    js.page = Some(rewrite.page);
                    js.invoke_path = Some(rewrite.pathname);
                    js.query = Some(rewrite.query_json);
                }
            }
        }
        js
    }
}

/// Resolves with the next request, or `null` once the executor shuts down.
#[op2]
#[serde]
async fn op_nextrs_next_request(state: Rc<RefCell<OpState>>) -> Option<JsRequest> {
    let inbox = state.borrow().borrow::<Inbox>().0.clone();
    let job = inbox.lock().await.recv().await?;
    job.response.mark_delivered();
    let mut state = state.borrow_mut();
    let worker_slot = state.borrow::<Arc<WorkerShared>>().slot;
    let requests = state.borrow_mut::<Requests>();
    requests.next_id = requests.next_id.wrapping_add(1);
    let id = requests.next_id;
    let request = job.request;
    let (body, body_stream) = match request.body {
        RequestBody::Empty => (None, None),
        // Vec::from(Bytes) reuses the allocation when it is not shared.
        RequestBody::Full(bytes) => (Some(Vec::from(bytes).into()), None),
        RequestBody::Stream(stream) => (None, Some(stream)),
    };
    let has_stream = body_stream.is_some();
    requests.open.insert(
        id,
        Open {
            flow: job.response.flow(),
            response: Some(job.response),
            body: body_stream,
            _slot: job.slot,
            delivered_at: Instant::now(),
        },
    );
    tracing::trace!(
        target: "nextrs::executor",
        slot = worker_slot,
        id,
        queued_us = job.queued_at.elapsed().as_micros() as u64,
        "request delivered"
    );
    Some(JsRequest {
        id,
        method: request.method,
        url: request.url,
        headers: request.headers,
        body,
        body_stream: has_stream,
        entry: request.entry.to_string_lossy().into_owned(),
        remote_address: request.remote_address,
        initial_headers: request.initial_headers,
        initial_status: request.initial_status,
        timeout_ms: job.timeout_ms,
        invocation: request.invocation.into(),
    })
}

/// The next chunk of a streamed request body, or `null` at its end. Errors
/// carry a Node-style code as the message prefix.
#[op2]
#[serde]
async fn op_nextrs_request_read(
    state: Rc<RefCell<OpState>>,
    #[smi] id: u32,
) -> Result<Option<ToJsBuffer>, JsErrorBox> {
    let stream = state
        .borrow_mut()
        .borrow_mut::<Requests>()
        .open
        .get_mut(&id)
        .and_then(|open| open.body.take());
    let Some(mut stream) = stream else {
        return Ok(None);
    };
    match stream.next().await {
        Some(Ok(chunk)) => {
            if let Some(open) = state
                .borrow_mut()
                .borrow_mut::<Requests>()
                .open
                .get_mut(&id)
            {
                open.body = Some(stream);
            }
            Ok(Some(Vec::from(chunk).into()))
        }
        Some(Err(error)) => {
            state
                .borrow()
                .borrow::<Arc<Counters>>()
                .count(metrics::COUNT_REQUEST_BODY_ERROR);
            let code = match error {
                BodyError::TooLarge { .. } => "ERR_NEXTRS_BODY_TOO_LARGE",
                BodyError::Aborted(_) => "ECONNRESET",
            };
            Err(JsErrorBox::generic(format!("{code}: {error}")))
        }
        None => Ok(None),
    }
}

/// The handler and its `waitUntil` work are finished (or abandoned):
/// releases the request's slot.
#[op2(fast)]
fn op_nextrs_request_done(state: &mut OpState, #[smi] id: u32) {
    let Some(open) = state.borrow_mut::<Requests>().open.remove(&id) else {
        return;
    };
    let counters = state.borrow::<Arc<Counters>>();
    metrics::add(&counters.requests_completed, 1);
    metrics::add(
        &counters.execution_micros,
        open.delivered_at.elapsed().as_micros() as u64,
    );
}

fn response(state: &mut OpState, id: u32) -> Option<&ResponseSender> {
    state
        .borrow::<Requests>()
        .open
        .get(&id)
        .and_then(|open| open.response.as_ref())
}

/// Returns false if the client is gone.
#[op2]
fn op_nextrs_response_head(
    state: &mut OpState,
    #[smi] id: u32,
    #[smi] status: u16,
    #[serde] headers: Vec<(String, String)>,
) -> bool {
    response(state, id).is_some_and(|r| r.head(status, headers))
}

/// Offers one body chunk; returns a `WriteStatus`.
#[op2(fast)]
#[smi]
fn op_nextrs_response_write(state: &mut OpState, #[smi] id: u32, #[buffer] chunk: &[u8]) -> u32 {
    let status = response(state, id).map_or(WriteStatus::Closed, |r| r.write(chunk));
    status as u32
}

/// Resolves true once the response is back under its buffer budget, false
/// if the consumer went away (client disconnect or cancellation).
#[op2]
async fn op_nextrs_response_drain(state: Rc<RefCell<OpState>>, #[smi] id: u32) -> bool {
    let flow = {
        let state = state.borrow();
        let Some(open) = state.borrow::<Requests>().open.get(&id) else {
            return false;
        };
        let counters = state.borrow::<Arc<Counters>>();
        metrics::add(&counters.backpressure_waits, 1);
        open.flow.clone()
    };
    flow.drained().await
}

fn finish(state: &mut OpState, id: u32, event: ResponseEvent) {
    let sender = state
        .borrow_mut::<Requests>()
        .open
        .get_mut(&id)
        .and_then(|open| open.response.take());
    if let Some(sender) = sender {
        sender.finish(event);
    }
}

#[op2(fast)]
fn op_nextrs_response_end(state: &mut OpState, #[smi] id: u32) {
    finish(state, id, ResponseEvent::End);
}

#[op2(fast)]
fn op_nextrs_response_error(state: &mut OpState, #[smi] id: u32, #[string] message: &str) {
    finish(state, id, ResponseEvent::Error(message.to_owned()));
}

/// Resolves when the consumer of the response (the HTTP connection) has
/// dropped it: after the body was delivered, or when the client went away
/// or the request was cancelled.
#[op2]
async fn op_nextrs_response_closed(state: Rc<RefCell<OpState>>, #[smi] id: u32) {
    let flow = state
        .borrow()
        .borrow::<Requests>()
        .open
        .get(&id)
        .map(|open| open.flow.clone());
    if let Some(flow) = flow {
        flow.closed().await;
    }
}

/// Fault injection for the supervision tests. Without the
/// `fault-injection` feature it only throws.
#[op2(fast)]
fn op_nextrs_fault(state: &mut OpState, #[string] kind: &str) -> Result<(), JsErrorBox> {
    #[cfg(feature = "fault-injection")]
    {
        match kind {
            // A Rust panic on the isolate thread, outside any V8 frame.
            "panic" => state.borrow::<Arc<WorkerShared>>().fault_panic.notify_one(),
            // Every open response channel closes, as if the Rust side of
            // the executor had lost them.
            "drop-responses" => {
                for open in state.borrow_mut::<Requests>().open.values_mut() {
                    open.response.take();
                }
            }
            other => return Err(JsErrorBox::generic(format!("unknown fault '{other}'"))),
        }
        Ok(())
    }
    #[cfg(not(feature = "fault-injection"))]
    {
        let _ = state;
        Err(JsErrorBox::generic(format!(
            "ERR_NEXTRS_UNSUPPORTED: fault injection ('{kind}') is not built in"
        )))
    }
}

fn fs_error(denied: FsDenied, syscall: &str, path: &str) -> JsErrorBox {
    let reason = match denied {
        FsDenied::OutsideRoots => "permission denied (outside NextRS read roots)",
        FsDenied::NotFound => "no such file or directory",
    };
    JsErrorBox::generic(format!("{denied}: {reason}, {syscall} '{path}'"))
}

fn io_error(error: &std::io::Error, syscall: &str, path: &str) -> JsErrorBox {
    let code = match error.kind() {
        std::io::ErrorKind::NotFound => "ENOENT",
        std::io::ErrorKind::IsADirectory => "EISDIR",
        std::io::ErrorKind::NotADirectory => "ENOTDIR",
        std::io::ErrorKind::PermissionDenied => "EACCES",
        _ => "EIO",
    };
    JsErrorBox::generic(format!("{code}: {error}, {syscall} '{path}'"))
}

#[op2]
#[buffer]
fn op_nextrs_fs_read(state: &mut OpState, #[string] path: &str) -> Result<Vec<u8>, JsErrorBox> {
    let resolved = state
        .borrow::<FsPolicy>()
        .resolve_read(path)
        .map_err(|denied| fs_error(denied, "open", path))?;
    std::fs::read(&resolved).map_err(|error| io_error(&error, "open", path))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatInfo {
    kind: &'static str,
    size: u64,
    mtime_ms: f64,
    mode: u32,
}

/// `null` when the path does not exist inside the roots; EACCES outside.
#[op2]
#[serde]
fn op_nextrs_fs_stat(
    state: &mut OpState,
    #[string] path: &str,
) -> Result<Option<StatInfo>, JsErrorBox> {
    let resolved = match state.borrow::<FsPolicy>().resolve_read(path) {
        Ok(resolved) => resolved,
        Err(FsDenied::NotFound) => return Ok(None),
        Err(denied) => return Err(fs_error(denied, "stat", path)),
    };
    let metadata = std::fs::metadata(&resolved).map_err(|error| io_error(&error, "stat", path))?;
    let mtime_ms = metadata
        .modified()
        .ok()
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .map_or(0.0, |d| d.as_secs_f64() * 1000.0);
    use std::os::unix::fs::PermissionsExt;
    Ok(Some(StatInfo {
        kind: if metadata.is_dir() { "dir" } else { "file" },
        size: metadata.len(),
        mtime_ms,
        mode: metadata.permissions().mode(),
    }))
}

#[op2]
#[serde]
fn op_nextrs_fs_readdir(
    state: &mut OpState,
    #[string] path: &str,
) -> Result<Vec<(String, bool)>, JsErrorBox> {
    let resolved = state
        .borrow::<FsPolicy>()
        .resolve_read(path)
        .map_err(|denied| fs_error(denied, "scandir", path))?;
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&resolved).map_err(|e| io_error(&e, "scandir", path))? {
        let entry = entry.map_err(|e| io_error(&e, "scandir", path))?;
        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
        entries.push((entry.file_name().to_string_lossy().into_owned(), is_dir));
    }
    entries.sort();
    Ok(entries)
}

#[op2]
#[string]
fn op_nextrs_fs_realpath(state: &mut OpState, #[string] path: &str) -> Result<String, JsErrorBox> {
    state
        .borrow::<FsPolicy>()
        .resolve_read(path)
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|denied| fs_error(denied, "realpath", path))
}

#[op2]
#[buffer]
fn op_nextrs_hash(#[string] algorithm: &str, #[buffer] data: &[u8]) -> Result<Vec<u8>, JsErrorBox> {
    use sha2::Digest;
    Ok(match algorithm {
        "md5" => md5::Md5::digest(data).to_vec(),
        "sha1" => sha1::Sha1::digest(data).to_vec(),
        "sha256" => sha2::Sha256::digest(data).to_vec(),
        "sha384" => sha2::Sha384::digest(data).to_vec(),
        "sha512" => sha2::Sha512::digest(data).to_vec(),
        other => {
            return Err(JsErrorBox::generic(format!(
                "ERR_NEXTRS_UNSUPPORTED: digest algorithm '{other}' is not supported"
            )));
        }
    })
}

#[op2(fast)]
fn op_nextrs_random_fill(#[buffer] buf: &mut [u8]) -> Result<(), JsErrorBox> {
    getrandom::fill(buf).map_err(|e| JsErrorBox::generic(format!("getrandom failed: {e}")))
}

/// `vm.runInNewContext` for scripts that only assign data onto their global,
/// which is how Next.js evaluates `*_client-reference-manifest.js`.
///
/// Node contextifies the sandbox object itself (interceptors forward every
/// global access to it). This creates a real, separate V8 context, copies
/// the sandbox's own enumerable properties onto its global, runs the script,
/// and copies the global's own enumerable properties back. Scripts that
/// later read the sandbox through closures see a copy instead.
///
/// Returns `[result, exception]`; bootstrap.js rethrows the exception.
#[op2]
fn op_nextrs_vm_run_in_new_context<'s>(
    scope: &mut v8::PinScope<'s, '_>,
    #[string] code: String,
    #[string] filename: String,
    sandbox: v8::Local<'s, v8::Object>,
) -> v8::Local<'s, v8::Value> {
    let out = v8::Array::new(scope, 2);
    let outer = scope.get_current_context();
    let token = outer.get_security_token(scope);
    let context = v8::Context::new(scope, Default::default());
    // Same token as the main context, as Node does for vm contexts, so
    // objects can cross between the two without access-check failures.
    context.set_security_token(token);
    let scope = &mut v8::ContextScope::new(scope, context);
    let global = context.global(scope);
    copy_own_properties(scope, sandbox, global);

    v8::tc_scope!(let tc, scope);
    let (Some(source), Some(name)) = (v8::String::new(tc, &code), v8::String::new(tc, &filename))
    else {
        return out.into();
    };
    let origin = v8::ScriptOrigin::new(
        tc,
        name.into(),
        0,
        0,
        false,
        0,
        None,
        false,
        false,
        false,
        None,
    );
    let result = v8::Script::compile(tc, source, Some(&origin)).and_then(|s| s.run(tc));
    match result {
        Some(value) => {
            copy_own_properties(tc, global, sandbox);
            out.set_index(tc, 0, value);
        }
        None => {
            if let Some(exception) = tc.exception() {
                out.set_index(tc, 1, exception);
            }
        }
    }
    out.into()
}

fn copy_own_properties(
    scope: &mut v8::PinScope<'_, '_>,
    from: v8::Local<'_, v8::Object>,
    to: v8::Local<'_, v8::Object>,
) {
    let Some(keys) = from.get_own_property_names(scope, Default::default()) else {
        return;
    };
    for i in 0..keys.length() {
        let Some(key) = keys.get_index(scope, i) else {
            continue;
        };
        if let Some(value) = from.get(scope, key) {
            to.set(scope, key, value);
        }
    }
}

// ---------------------------------------------------------------------------
// Outbound fetch. JavaScript errors carry `CODE: message`; 01_fetch.js turns
// them into Node-style `TypeError("fetch failed", { cause })`.
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FetchArgs {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    /// "none", "bytes" (the buffer argument) or "stream" (written with
    /// `op_nextrs_fetch_write`).
    body_kind: String,
    redirect: String,
    /// Next's `next: { internal }` marker: a request to its own server.
    #[serde(default)]
    internal: bool,
}

fn aborted() -> JsErrorBox {
    JsErrorBox::generic("ABORT_ERR: the fetch was aborted")
}

/// Starts a fetch; returns its id. Refused at once (no network task) when
/// the network is not configured, the URL is not allowed, or the isolate
/// already holds its limit of open fetches.
#[op2]
#[smi]
fn op_nextrs_fetch_start(
    state: &mut OpState,
    #[serde] args: FetchArgs,
    #[buffer(copy)] body: Vec<u8>,
) -> Result<u32, JsErrorBox> {
    if args.internal
        && let Ok(url) = url::Url::parse(&args.url)
        && crate::internal::InternalRoute::handles(&url)
    {
        return start_internal(state, args, url, body);
    }
    let Some(net) = state.borrow::<NetCapability>().0.clone() else {
        return Err(JsErrorBox::generic(
            "ERR_NEXTRS_NET_DENIED: outbound network access is not enabled (see --net-allow)",
        ));
    };
    let limit = net.limits().max_in_flight_per_isolate;
    if state.borrow::<Fetches>().open.len() >= limit {
        return Err(JsErrorBox::generic(format!(
            "ERR_NEXTRS_NET_LIMIT: this isolate already has {limit} outbound fetches open"
        )));
    }
    let invalid = |what: &str| JsErrorBox::generic(format!("ERR_INVALID_ARG_VALUE: {what}"));
    let method =
        reqwest::Method::from_bytes(args.method.as_bytes()).map_err(|_| invalid("method"))?;
    let url = url::Url::parse(&args.url).map_err(|_| invalid("URL"))?;
    let (body, upload) = match args.body_kind.as_str() {
        "none" => (OutBody::None, None),
        "bytes" => (OutBody::Bytes(Bytes::from(body)), None),
        "stream" => {
            let (tx, rx) = mpsc::channel(1);
            (OutBody::Stream(rx), Some(tx))
        }
        _ => return Err(invalid("body kind")),
    };
    let redirect = match args.redirect.as_str() {
        "manual" => RedirectMode::Manual,
        "error" => RedirectMode::Error,
        _ => RedirectMode::Follow,
    };
    let task = net
        .start(FetchRequest {
            method,
            url,
            headers: args.headers,
            body,
            redirect,
        })
        .map_err(|error| JsErrorBox::generic(error.to_js()))?;
    let counters = state.borrow::<Arc<Counters>>().clone();
    metrics::add(&counters.fetch_in_flight, 1);
    let fetches = state.borrow_mut::<Fetches>();
    fetches.next_id = fetches.next_id.wrapping_add(1);
    let id = fetches.next_id;
    fetches.open.insert(
        id,
        OpenFetch {
            head: Some(task.head),
            body: Some(task.body),
            upload,
            abort: task.abort,
            settled: false,
            counters,
        },
    );
    Ok(id)
}

/// Next.js fetching its own server (see internal.rs): served by the HTTP
/// layer in-process, with the same per-isolate fetch limit.
fn start_internal(
    state: &mut OpState,
    args: FetchArgs,
    url: url::Url,
    body: Vec<u8>,
) -> Result<u32, JsErrorBox> {
    let limit = state.borrow::<NetCapability>().0.as_ref().map_or(
        crate::net::NetworkLimits::default().max_in_flight_per_isolate,
        |net| net.limits().max_in_flight_per_isolate,
    );
    if state.borrow::<Fetches>().open.len() >= limit {
        return Err(JsErrorBox::generic(format!(
            "ERR_NEXTRS_NET_LIMIT: this isolate already has {limit} outbound fetches open"
        )));
    }
    let (body, upload) = match args.body_kind.as_str() {
        "none" => (OutBody::None, None),
        "bytes" => (OutBody::Bytes(Bytes::from(body)), None),
        "stream" => {
            let (tx, rx) = mpsc::channel(1);
            (OutBody::Stream(rx), Some(tx))
        }
        _ => {
            return Err(JsErrorBox::generic(
                "ERR_INVALID_ARG_VALUE: body kind".to_owned(),
            ));
        }
    };
    let route = state.borrow::<crate::internal::InternalRoute>().clone();
    let task = route
        .start(args.method, &url, args.headers, body)
        .map_err(|error| JsErrorBox::generic(error.to_js()))?;
    let counters = state.borrow::<Arc<Counters>>().clone();
    metrics::add(&counters.fetch_in_flight, 1);
    let fetches = state.borrow_mut::<Fetches>();
    fetches.next_id = fetches.next_id.wrapping_add(1);
    let id = fetches.next_id;
    fetches.open.insert(
        id,
        OpenFetch {
            head: Some(task.head),
            body: Some(task.body),
            upload,
            abort: task.abort,
            settled: false,
            counters,
        },
    );
    Ok(id)
}

/// The response head, once it arrives (after any redirects).
#[op2]
#[serde]
async fn op_nextrs_fetch_head(
    state: Rc<RefCell<OpState>>,
    #[smi] id: u32,
) -> Result<FetchHead, JsErrorBox> {
    let head = state
        .borrow_mut()
        .borrow_mut::<Fetches>()
        .open
        .get_mut(&id)
        .and_then(|fetch| fetch.head.take());
    let Some(head) = head else {
        return Err(aborted());
    };
    match head.await {
        Ok(Ok(head)) => Ok(head),
        Ok(Err(error)) => {
            if let Some(fetch) = state.borrow_mut().borrow_mut::<Fetches>().open.get_mut(&id) {
                fetch.settled = true;
            }
            Err(JsErrorBox::generic(error.to_js()))
        }
        Err(_) => Err(aborted()),
    }
}

/// The next chunk of the response body, or `null` at its end. The network
/// task reads from the socket only as JavaScript pulls.
#[op2]
#[serde]
async fn op_nextrs_fetch_read(
    state: Rc<RefCell<OpState>>,
    #[smi] id: u32,
) -> Result<Option<ToJsBuffer>, JsErrorBox> {
    let body = state
        .borrow_mut()
        .borrow_mut::<Fetches>()
        .open
        .get_mut(&id)
        .and_then(|fetch| fetch.body.take());
    let Some(mut body) = body else {
        return Err(aborted());
    };
    let item = body.recv().await;
    let mut state = state.borrow_mut();
    let fetch = state.borrow_mut::<Fetches>().open.get_mut(&id);
    match item {
        Some(BodyItem::Chunk(chunk)) => {
            if let Some(fetch) = fetch {
                fetch.body = Some(body);
            }
            Ok(Some(Vec::from(chunk).into()))
        }
        Some(BodyItem::End) => {
            if let Some(fetch) = fetch {
                fetch.settled = true;
            }
            Ok(None)
        }
        Some(BodyItem::Error(error)) => {
            if let Some(fetch) = fetch {
                fetch.settled = true;
            }
            Err(JsErrorBox::generic(error.to_js()))
        }
        // The task ended without End: it was aborted.
        None => Err(aborted()),
    }
}

/// Writes one chunk of a streamed request body. Resolves when the network
/// task has taken it (at most one chunk is buffered); false if the request
/// no longer reads its body.
#[op2]
async fn op_nextrs_fetch_write(
    state: Rc<RefCell<OpState>>,
    #[smi] id: u32,
    #[buffer(copy)] chunk: Vec<u8>,
) -> bool {
    let upload = state
        .borrow()
        .borrow::<Fetches>()
        .open
        .get(&id)
        .and_then(|fetch| fetch.upload.clone());
    match upload {
        Some(upload) => upload.send(Bytes::from(chunk)).await.is_ok(),
        None => false,
    }
}

/// Ends a streamed request body.
#[op2(fast)]
fn op_nextrs_fetch_write_end(state: &mut OpState, #[smi] id: u32) {
    if let Some(fetch) = state.borrow_mut::<Fetches>().open.get_mut(&id) {
        fetch.upload = None;
    }
}

/// Releases a fetch; aborts it if it is still running. Idempotent.
#[op2(fast)]
fn op_nextrs_fetch_close(state: &mut OpState, #[smi] id: u32) {
    state.borrow_mut::<Fetches>().open.remove(&id);
}

// ---------------------------------------------------------------------------
// Shared data cache (cache.rs). Keys, tags and values only: JavaScript never
// names a file. Disk access runs on Tokio's blocking threads.
// ---------------------------------------------------------------------------

fn cache_store(state: &Rc<RefCell<OpState>>) -> Result<Arc<CacheStore>, JsErrorBox> {
    state
        .borrow()
        .borrow::<CacheCapability>()
        .0
        .clone()
        .ok_or_else(|| JsErrorBox::generic("ERR_NEXTRS_UNSUPPORTED: the shared cache is disabled"))
}

fn blocking_failed(error: tokio::task::JoinError) -> JsErrorBox {
    JsErrorBox::generic(format!("cache operation failed: {error}"))
}

/// The entry stored for `key` as stored, or `null` on a miss. Tags are
/// judged in JavaScript by Next's own functions (after `tag_sync`).
#[op2]
#[serde]
async fn op_nextrs_cache_get(
    state: Rc<RefCell<OpState>>,
    #[string] key: String,
) -> Result<Option<Hit>, JsErrorBox> {
    let store = cache_store(&state)?;
    match store.lookup(&key) {
        Lookup::Hit(hit) => Ok(Some(hit)),
        Lookup::Miss => Ok(None),
        Lookup::OnDisk { path, generation } => {
            tokio::task::spawn_blocking(move || store.load(&key, &path, generation))
                .await
                .map_err(blocking_failed)
        }
    }
}

/// Stores a value (`null` removes the entry). The value is in memory, and
/// visible to every isolate, as soon as this is called; the promise
/// resolves once it is persisted. `response` marks a response-cache entry
/// (ISR page or route handler output). `last_modified` (Unix ms, negative:
/// now) and `if_absent` serve build-time prerenders kept in the store.
#[op2]
fn op_nextrs_cache_set(
    state: Rc<RefCell<OpState>>,
    #[string] key: String,
    #[string] value: Option<String>,
    #[serde] tags: Vec<String>,
    response: bool,
    last_modified: f64,
    if_absent: bool,
) -> Result<impl std::future::Future<Output = Result<(), JsErrorBox>>, JsErrorBox> {
    let store = cache_store(&state)?;
    let kind = if response {
        EntryKind::Response
    } else {
        EntryKind::Fetch
    };
    let options = SetOptions {
        last_modified: (last_modified >= 0.0).then_some(last_modified as u64),
        if_absent,
    };
    let write = match value {
        None => {
            store.delete(&key);
            None
        }
        Some(value) => store.set_with(&key, Bytes::from(value), tags, kind, options),
    };
    Ok(async move {
        if let Some(write) = write {
            tokio::task::spawn_blocking(move || store.persist(write))
                .await
                .map_err(blocking_failed)?;
        }
        Ok(())
    })
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct LeaseResult {
    acquired: bool,
    id: f64,
    timed_out: bool,
}

/// Single flight for a blocking regeneration across isolates: takes the
/// lease for `key` (`acquired`, with the `id` to release) or waits up to
/// `ttl_ms` for the isolate that holds it.
#[op2]
#[serde]
async fn op_nextrs_cache_lease(
    state: Rc<RefCell<OpState>>,
    #[string] key: String,
    ttl_ms: f64,
) -> Result<LeaseResult, JsErrorBox> {
    let store = cache_store(&state)?;
    let owner = {
        let state = state.borrow();
        let worker = state.borrow::<Arc<WorkerShared>>();
        (worker.slot, worker.generation)
    };
    let ttl = std::time::Duration::from_millis(ttl_ms.clamp(1.0, 600_000.0) as u64);
    Ok(match store.acquire_lease(&key, ttl, Some(owner)).await {
        LeaseOutcome::Acquired(id) => LeaseResult {
            acquired: true,
            id: id as f64,
            timed_out: false,
        },
        LeaseOutcome::Waited { timed_out } => LeaseResult {
            acquired: false,
            id: 0.0,
            timed_out,
        },
    })
}

#[op2(fast)]
fn op_nextrs_cache_release(state: &mut OpState, #[string] key: &str, id: f64) {
    if let Some(store) = state.borrow::<CacheCapability>().0.as_ref() {
        store.release_lease(key, id as u64);
    }
}

/// Tag changes since `since` for this isolate's mirror of Next's tags
/// manifest (synchronous: memory only).
#[op2]
#[serde]
fn op_nextrs_cache_tag_sync(state: &mut OpState, since: f64) -> Result<TagSync, JsErrorBox> {
    let store = state.borrow::<CacheCapability>().0.clone().ok_or_else(|| {
        JsErrorBox::generic("ERR_NEXTRS_UNSUPPORTED: the shared cache is disabled")
    })?;
    Ok(store.tag_sync(since.max(0.0) as u64))
}

/// Next's `revalidateTag(tags, durations)`: `expire` is `durations.expire`
/// in seconds (negative: not given), `with_durations` whether durations
/// were given at all.
#[op2]
async fn op_nextrs_cache_revalidate(
    state: Rc<RefCell<OpState>>,
    #[serde] tags: Vec<String>,
    expire: f64,
    with_durations: bool,
) -> Result<(), JsErrorBox> {
    let store = cache_store(&state)?;
    let expire = (expire >= 0.0).then_some(expire);
    if let Some(contents) = store.revalidate(&tags, expire, with_durations) {
        tokio::task::spawn_blocking(move || store.persist_tags(contents))
            .await
            .map_err(blocking_failed)?;
    }
    Ok(())
}
