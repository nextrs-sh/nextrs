//! Dynamic routes rendered by the embedded V8 executor (`nextrs-runtime`).
//! The executor calls Next's compiled `handler(req, res, ctx)`; this module
//! only moves the request in and the streamed response out.

use std::path::Path;

use axum::body::{Body, Bytes};
use axum::http::header::{self, HeaderName, HeaderValue};
use axum::http::{HeaderMap, Method, StatusCode, Uri, Version};
use axum::response::{IntoResponse, Response};
use nextrs_runtime::{Executor, RenderRequest, ResponseEvent};
use tokio::sync::mpsc;

/// Request bodies are buffered before the handler runs.
const MAX_REQUEST_BODY: usize = 10 * 1024 * 1024;

/// What next start's base server does around every App Router render.
pub(crate) struct AppPathPolicy<'a> {
    /// Appended as `Vary` before the handler runs.
    pub vary: &'a str,
}

pub(crate) async fn render(
    executor: &Executor,
    entry: &Path,
    policy: &AppPathPolicy<'_>,
    method: &Method,
    uri: &Uri,
    headers: &HeaderMap,
    body: Body,
) -> Response {
    let body = match axum::body::to_bytes(body, MAX_REQUEST_BODY).await {
        Ok(body) => body,
        Err(_) => {
            return error_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                "request-body-too-large",
                "request body exceeds the 10 MiB NextRS limit",
            );
        }
    };
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
        body: body.to_vec(),
        entry: entry.to_path_buf(),
        remote_address: None,
        initial_headers: vec![("vary".to_owned(), policy.vary.to_owned())],
    };
    let mut events = match executor.render(request) {
        Ok(events) => events,
        Err(error) => {
            return error_response(
                StatusCode::BAD_GATEWAY,
                "executor-stopped",
                &error.to_string(),
            );
        }
    };
    let (status, response_headers) = match events.recv().await {
        Some(ResponseEvent::Head { status, headers }) => (status, headers),
        Some(ResponseEvent::Error(error)) => {
            return error_response(StatusCode::BAD_GATEWAY, "render-failed", &error);
        }
        Some(ResponseEvent::Chunk(_) | ResponseEvent::End) | None => {
            return error_response(
                StatusCode::BAD_GATEWAY,
                "render-failed",
                "the JavaScript executor ended the response without a status line",
            );
        }
    };

    let mut response = Response::new(Body::from_stream(body_stream(events)));
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

/// Chunks until `End`. An `Error`, or the executor going away mid-body,
/// aborts the HTTP response instead of ending it cleanly, so a truncated
/// render is never presented as complete. Dropping the stream (client
/// disconnect) drops the receiver, which JavaScript observes as `close`.
fn body_stream(
    events: mpsc::UnboundedReceiver<ResponseEvent>,
) -> impl futures_util::Stream<Item = Result<Bytes, std::io::Error>> {
    futures_util::stream::unfold(Some(events), |state| async move {
        let mut events = state?;
        loop {
            match events.recv().await {
                Some(ResponseEvent::Chunk(chunk)) => {
                    return Some((Ok(Bytes::from(chunk)), Some(events)));
                }
                Some(ResponseEvent::Head { .. }) => continue,
                Some(ResponseEvent::End) => return None,
                Some(ResponseEvent::Error(error)) => {
                    tracing::warn!(%error, "aborting response after handler failure");
                    return Some((Err(std::io::Error::other(error)), None));
                }
                None => {
                    return Some((
                        Err(std::io::Error::other(
                            "JavaScript executor stopped mid-response",
                        )),
                        None,
                    ));
                }
            }
        }
    })
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
