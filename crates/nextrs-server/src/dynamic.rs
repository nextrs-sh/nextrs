//! Dynamic routes rendered by the embedded V8 executor (`nextrs-runtime`).
//! The executor calls Next's compiled `handler(req, res, ctx)`; this module
//! only moves the request in and the streamed response out.
//!
//! Neither direction is buffered whole: the request body is pulled from the
//! connection as JavaScript reads it (counting bytes against the limit,
//! whatever the framing), and the response is flow-controlled by the
//! executor's per-response budget, which stays charged until hyper has
//! written each chunk to the socket.

use std::net::SocketAddr;
use std::path::Path;
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::http::header::{self, HeaderName, HeaderValue};
use axum::http::{HeaderMap, Method, StatusCode, Uri, Version};
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use http_body::Body as _;
use nextrs_runtime::{
    BodyError, BodyStream, Executor, RenderError, RenderRequest, RequestBody, ResponseEvent,
    ResponseStream,
};
use tokio::sync::oneshot;
use tokio::time::Instant;

/// Request bodies above this are refused (413).
pub const DEFAULT_MAX_REQUEST_BODY: u64 = 10 * 1024 * 1024;
/// How long past the executor's own request deadline the HTTP layer waits
/// before giving up on an isolate that does not answer at all (for example
/// one stuck on the CPU until the stall watchdog replaces it).
const BACKSTOP_GRACE: Duration = Duration::from_secs(5);

/// What next start's base server does around every App Router render.
pub(crate) struct AppPathPolicy<'a> {
    /// Appended as `Vary` before the handler runs.
    pub vary: &'a str,
}

pub(crate) struct Incoming<'a> {
    pub method: &'a Method,
    pub uri: &'a Uri,
    pub headers: &'a HeaderMap,
    pub body: Body,
    pub client: Option<SocketAddr>,
    pub max_body: u64,
}

pub(crate) async fn render(
    executor: &Executor,
    entry: &Path,
    policy: &AppPathPolicy<'_>,
    incoming: Incoming<'_>,
) -> Response {
    let Incoming {
        method,
        uri,
        headers,
        body,
        client,
        max_body,
    } = incoming;
    // A declared length over the limit is refused before any JavaScript
    // runs. Chunked and unknown-length bodies are counted as they stream.
    if content_length(headers).is_some_and(|length| length > max_body) {
        return too_large(max_body);
    }
    let (body, overflow) = request_body(body, max_body);
    let request = RenderRequest {
        method: method.as_str().to_owned(),
        url: uri
            .path_and_query()
            .map_or_else(|| uri.path().to_owned(), |pq| pq.as_str().to_owned()),
        // Fixture builds have no interception routes, so next start drops
        // Next-URL before rendering; responses then cannot depend on it.
        headers: headers
            .iter()
            .filter(|(name, _)| *name != "next-url")
            .map(|(name, value)| {
                (
                    name.as_str().to_owned(),
                    String::from_utf8_lossy(value.as_bytes()).into_owned(),
                )
            })
            .collect(),
        body,
        entry: entry.to_path_buf(),
        remote_address: client.map(|addr| addr.ip().to_string()),
        initial_headers: vec![("vary".to_owned(), policy.vary.to_owned())],
    };
    let mut events = match executor.render(request).await {
        Ok(events) => events,
        Err(error) => return refused(error),
    };
    let deadline = executor
        .limits()
        .request_timeout
        .map(|timeout| Instant::now() + timeout + BACKSTOP_GRACE);

    // An overflowing body wins over a handler that has not answered yet:
    // dropping `events` cancels the request in JavaScript.
    let first = tokio::select! {
        biased;
        () = overflowed(overflow) => return too_large(max_body),
        event = next_event(&mut events, deadline) => event,
    };
    let (status, response_headers) = match first {
        Ok(Some(ResponseEvent::Head { status, headers })) => (status, headers),
        Ok(Some(ResponseEvent::Error(error))) => {
            return error_response(StatusCode::BAD_GATEWAY, "render-failed", &error);
        }
        Ok(Some(ResponseEvent::Chunk(_) | ResponseEvent::End)) => {
            return error_response(
                StatusCode::BAD_GATEWAY,
                "render-failed",
                "the JavaScript executor ended the response without a status line",
            );
        }
        Ok(None) => return executor_gone(&events),
        Err(Elapsed) => {
            return error_response(
                StatusCode::GATEWAY_TIMEOUT,
                "request-timeout",
                "the JavaScript isolate did not answer before the request deadline",
            );
        }
    };

    let mut response = Response::new(Body::from_stream(body_stream(events, deadline)));
    *response.status_mut() =
        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let map = response.headers_mut();
    for (name, value) in response_headers {
        match (
            HeaderName::from_bytes(name.as_bytes()),
            HeaderValue::from_str(&value),
        ) {
            (Ok(name), Ok(value)) => {
                map.append(name, value);
            }
            _ => tracing::warn!(header = %name, "dropping invalid response header from handler"),
        }
    }
    response
}

fn content_length(headers: &HeaderMap) -> Option<u64> {
    headers
        .get(header::CONTENT_LENGTH)?
        .to_str()
        .ok()?
        .parse()
        .ok()
}

/// The request body as a pull stream that fails with `TooLarge` once more
/// than `limit` bytes arrived, and signals `overflow` when it does.
fn request_body(body: Body, limit: u64) -> (RequestBody, Option<oneshot::Receiver<()>>) {
    if body.is_end_stream() {
        return (RequestBody::Empty, None);
    }
    let (overflow_tx, overflow_rx) = oneshot::channel();
    struct State {
        data: axum::body::BodyDataStream,
        seen: u64,
        limit: u64,
        overflow: Option<oneshot::Sender<()>>,
        done: bool,
    }
    let state = State {
        data: body.into_data_stream(),
        seen: 0,
        limit,
        overflow: Some(overflow_tx),
        done: false,
    };
    let stream: BodyStream = Box::pin(futures_util::stream::unfold(
        state,
        |mut state| async move {
            if state.done {
                return None;
            }
            let item = match state.data.next().await? {
                Ok(chunk) => {
                    state.seen += chunk.len() as u64;
                    if state.seen > state.limit {
                        state.done = true;
                        if let Some(overflow) = state.overflow.take() {
                            let _ = overflow.send(());
                        }
                        Err(BodyError::TooLarge { limit: state.limit })
                    } else {
                        Ok(chunk)
                    }
                }
                Err(error) => {
                    state.done = true;
                    Err(BodyError::Aborted(error.to_string()))
                }
            };
            Some((item, state))
        },
    ));
    (RequestBody::Stream(stream), Some(overflow_rx))
}

async fn overflowed(overflow: Option<oneshot::Receiver<()>>) {
    if let Some(receiver) = overflow
        && receiver.await.is_ok()
    {
        return;
    }
    std::future::pending().await
}

struct Elapsed;

async fn next_event(
    events: &mut ResponseStream,
    deadline: Option<Instant>,
) -> Result<Option<ResponseEvent>, Elapsed> {
    match deadline {
        Some(deadline) => tokio::time::timeout_at(deadline, events.recv())
            .await
            .map_err(|_| Elapsed),
        None => Ok(events.recv().await),
    }
}

/// Chunks until `End`. An `Error`, the executor going away, or the backstop
/// deadline mid-body aborts the HTTP response instead of ending it cleanly,
/// so a truncated render is never presented as complete. Dropping the
/// stream (client disconnect) cancels the request in JavaScript.
fn body_stream(
    events: ResponseStream,
    deadline: Option<Instant>,
) -> impl futures_util::Stream<Item = Result<Bytes, std::io::Error>> {
    futures_util::stream::unfold(Some(events), move |state| async move {
        let mut events = state?;
        loop {
            let failure = match next_event(&mut events, deadline).await {
                Ok(Some(ResponseEvent::Chunk(chunk))) => {
                    return Some((Ok(chunk), Some(events)));
                }
                Ok(Some(ResponseEvent::Head { .. })) => continue,
                Ok(Some(ResponseEvent::End)) => return None,
                Ok(Some(ResponseEvent::Error(error))) => {
                    tracing::warn!(%error, "aborting response after handler failure");
                    error
                }
                Ok(None) => "JavaScript executor stopped mid-response".to_owned(),
                Err(Elapsed) => "request deadline exceeded mid-response".to_owned(),
            };
            return Some((Err(std::io::Error::other(failure)), None));
        }
    })
}

/// The stream ended before a status line. A request JavaScript never
/// received is safe to retry (503); one it did receive may have run (502).
fn executor_gone(events: &ResponseStream) -> Response {
    if events.was_delivered() {
        error_response(
            StatusCode::BAD_GATEWAY,
            "executor-failed",
            "the JavaScript isolate failed while handling the request",
        )
    } else {
        retry_later(
            "executor-unavailable",
            "the JavaScript isolate failed before the request reached it",
        )
    }
}

/// Requests refused before reaching JavaScript: 503 with `Retry-After`,
/// which tells clients a retry is safe whatever the method.
fn refused(error: RenderError) -> Response {
    retry_later(error.code(), &error.to_string())
}

fn retry_later(code: &'static str, message: &str) -> Response {
    let mut response = error_response(StatusCode::SERVICE_UNAVAILABLE, code, message);
    response
        .headers_mut()
        .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
    response
}

fn too_large(limit: u64) -> Response {
    let mut response = error_response(
        StatusCode::PAYLOAD_TOO_LARGE,
        "request-body-too-large",
        &format!("request body exceeds the {limit}-byte NextRS limit"),
    );
    // The rest of the body is not read; the connection cannot be reused.
    response
        .headers_mut()
        .insert(header::CONNECTION, HeaderValue::from_static("close"));
    response
}

/// HTTP/1.0 has no chunked encoding: a streamed body without a length is
/// delimited by closing the connection, and Node sends `Connection: close`.
/// hyper decides keep-alive from the *response* version, which defaults to
/// HTTP/1.1, so it must be set to 1.0 for hyper to close.
pub(crate) fn frame_for(version: Version, mut response: Response) -> Response {
    if version <= Version::HTTP_10 && !response.headers().contains_key(header::CONTENT_LENGTH) {
        *response.version_mut() = Version::HTTP_10;
        response
            .headers_mut()
            .insert(header::CONNECTION, HeaderValue::from_static("close"));
    }
    response
}

fn error_response(status: StatusCode, code: &'static str, message: &str) -> Response {
    (
        status,
        [
            ("content-type", "text/plain; charset=utf-8"),
            ("cache-control", "no-store"),
            ("x-nextrs-error", code),
        ],
        format!("NextRS: {message}\n"),
    )
        .into_response()
}
