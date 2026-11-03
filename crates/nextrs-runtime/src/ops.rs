//! The only capabilities JavaScript gets from the host. Everything a Next.js
//! handler can do outside the V8 heap goes through one of these ops.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::time::UNIX_EPOCH;

use deno_core::{OpState, ToJsBuffer, op2, v8};
use deno_error::JsErrorBox;
use serde::Serialize;
use tokio::sync::mpsc;

use crate::sandbox::{FsDenied, FsPolicy};
use crate::{Job, ResponseEvent};

/// Static configuration handed to bootstrap.js.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct BootConfig {
    pub project_dir: String,
    pub dist_dir: String,
    pub env: BTreeMap<String, String>,
    pub node_version: &'static str,
    pub nextrs_version: &'static str,
}

pub(crate) struct Inbox(pub Rc<tokio::sync::Mutex<mpsc::UnboundedReceiver<Job>>>);

#[derive(Default)]
pub(crate) struct Responses {
    next_id: u32,
    open: HashMap<u32, mpsc::UnboundedSender<ResponseEvent>>,
}

pub(crate) struct ReadySignal(pub Option<std::sync::mpsc::Sender<Result<(), String>>>);

deno_core::extension!(
    nextrs_runtime,
    deps = [deno_webidl, deno_web],
    ops = [
        op_nextrs_boot_config,
        op_nextrs_ready,
        op_nextrs_diagnostic,
        op_nextrs_next_request,
        op_nextrs_response_head,
        op_nextrs_response_write,
        op_nextrs_response_end,
        op_nextrs_response_error,
        op_nextrs_response_closed,
        op_nextrs_fs_read,
        op_nextrs_fs_stat,
        op_nextrs_fs_readdir,
        op_nextrs_fs_realpath,
        op_nextrs_hash,
        op_nextrs_random_fill,
        op_nextrs_vm_run_in_new_context,
    ],
    options = {
        boot: BootConfig,
        policy: FsPolicy,
        inbox: Inbox,
        ready: ReadySignal,
    },
    state = |state, options| {
        state.put(options.boot);
        state.put(options.policy);
        state.put(options.inbox);
        state.put(options.ready);
        state.put(Responses::default());
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

/// Unsupported APIs and missing built-in members, reported by bootstrap.js.
#[op2(fast)]
fn op_nextrs_diagnostic(#[string] kind: &str, #[string] detail: &str) {
    tracing::warn!(target: "nextrs::js", kind, "{detail}");
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct JsRequest {
    id: u32,
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: Option<ToJsBuffer>,
    entry: String,
    remote_address: Option<String>,
}

/// Resolves with the next request, or `null` once the executor shuts down.
#[op2]
#[serde]
async fn op_nextrs_next_request(state: Rc<RefCell<OpState>>) -> Option<JsRequest> {
    let inbox = state.borrow().borrow::<Inbox>().0.clone();
    let job = inbox.lock().await.recv().await?;
    let mut state = state.borrow_mut();
    let responses = state.borrow_mut::<Responses>();
    responses.next_id = responses.next_id.wrapping_add(1);
    let id = responses.next_id;
    responses.open.insert(id, job.events);
    let request = job.request;
    Some(JsRequest {
        id,
        method: request.method,
        url: request.url,
        headers: request.headers,
        body: (!request.body.is_empty()).then(|| request.body.into()),
        entry: request.entry.to_string_lossy().into_owned(),
        remote_address: request.remote_address,
    })
}

fn send(state: &mut OpState, id: u32, event: ResponseEvent) -> bool {
    let responses = state.borrow_mut::<Responses>();
    match responses.open.get(&id) {
        Some(sender) => sender.send(event).is_ok(),
        None => false,
    }
}

/// Returns false if the client is gone.
#[op2]
fn op_nextrs_response_head(
    state: &mut OpState,
    #[smi] id: u32,
    #[smi] status: u16,
    #[serde] headers: Vec<(String, String)>,
) -> bool {
    send(state, id, ResponseEvent::Head { status, headers })
}

#[op2(fast)]
fn op_nextrs_response_write(state: &mut OpState, #[smi] id: u32, #[buffer] chunk: &[u8]) -> bool {
    send(state, id, ResponseEvent::Chunk(chunk.to_vec()))
}

#[op2(fast)]
fn op_nextrs_response_end(state: &mut OpState, #[smi] id: u32) {
    send(state, id, ResponseEvent::End);
    state.borrow_mut::<Responses>().open.remove(&id);
}

#[op2(fast)]
fn op_nextrs_response_error(state: &mut OpState, #[smi] id: u32, #[string] message: &str) {
    send(state, id, ResponseEvent::Error(message.to_owned()));
    state.borrow_mut::<Responses>().open.remove(&id);
}

/// Resolves when the consumer of the response (the HTTP connection) has
/// dropped it: after the body was delivered, or when the client went away.
#[op2]
async fn op_nextrs_response_closed(state: Rc<RefCell<OpState>>, #[smi] id: u32) {
    let sender = state.borrow().borrow::<Responses>().open.get(&id).cloned();
    if let Some(sender) = sender {
        sender.closed().await;
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
