//! External rewrites: a `next.config` rewrite or a `NextResponse.rewrite()`
//! whose destination is another origin. next start proxies these
//! (router-server's `proxyRequest`, Next's bundled httpxy); NextRS forwards
//! them through the runtime's network capability (`nextrs_runtime::Upstream`),
//! so the destination must pass the same policy as `fetch`: `--net-allow`
//! names the origins, `--net-allow-addr` the non-public addresses, and the
//! resolver checks every address a connection uses (DNS rebinding included).
//! Without a policy, or for a destination it refuses, the request gets a 502
//! and nothing is sent.
//!
//! What is forwarded, as httpxy does with next start's options
//! (`changeOrigin`, `ignorePath`, no `xfwd`, no redirect following):
//!
//! * the method, the destination URL (its query re-stringified by routing),
//!   and the request headers as routing left them (`Host` becomes the
//!   destination's; `X-Forwarded-Host` becomes the client's `Host`);
//! * the request body (the proxy's buffered copy, or the client's stream);
//! * back: the status, every response header (`Set-Cookie` included,
//!   unchanged) and the body, streamed with backpressure. A 3xx is returned,
//!   not followed.
//!
//! Deliberate differences from next start: hop-by-hop headers (RFC 9110
//! 7.6.1) and Next's internal routing headers are not sent upstream (next
//! start forwards `x-middleware-set-cookie` and the like); request bodies
//! above `--max-request-body` get a 413; WebSocket upgrades are refused.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use axum::body::{Body, Bytes};
use axum::http::header::{self, HeaderName, HeaderValue};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::{IntoResponse, Response};
use futures_util::StreamExt;
use nextrs_runtime::{Upstream, UpstreamBody, UpstreamRequest};

use crate::dynamic;

/// Hop-by-hop headers (RFC 9110 7.6.1, plus the legacy `Keep-Alive` and
/// `Proxy-Connection`), never forwarded in either direction.
const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-connection",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// The request to forward, as routing left it.
pub(crate) struct Forward<'a> {
    pub url: &'a str,
    pub method: &'a Method,
    /// The request headers the route would see (after the proxy's changes).
    pub headers: &'a HeaderMap,
    /// The client's own `Host` header (before any routing change).
    pub client_host: Option<&'a HeaderValue>,
    pub body: Body,
    pub max_body: u64,
    /// `experimental.proxyTimeout`: how long the upstream may take to
    /// answer, and to send each further part of its body (httpxy sets it as
    /// the upstream socket's idle timeout). `None`: no limit.
    pub timeout: Option<std::time::Duration>,
}

pub(crate) fn denied(message: &str) -> Response {
    dynamic::error_response(StatusCode::BAD_GATEWAY, "external-rewrite-denied", message)
}

/// next start's answer when the proxy fails before a response: 500 with a
/// plain body and no content type.
fn proxy_error() -> Response {
    let mut response = (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error").into_response();
    response.headers_mut().remove(header::CONTENT_TYPE);
    response
}

/// Removes hop-by-hop headers and the ones the `Connection` header names.
fn strip_hop_by_hop(headers: &mut HeaderMap) {
    let named: Vec<HeaderName> = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .filter_map(|name| HeaderName::from_bytes(name.trim().as_bytes()).ok())
        .collect();
    for name in named {
        headers.remove(name);
    }
    for name in HOP_BY_HOP {
        headers.remove(*name);
    }
}

fn request_headers(forward: &Forward<'_>, streamed: bool) -> HeaderMap {
    let mut headers = forward.headers.clone();
    strip_hop_by_hop(&mut headers);
    headers.remove(header::HOST);
    headers.remove(header::EXPECT);
    for name in crate::resolve::NEXT_INTERNAL_HEADERS {
        headers.remove(*name);
    }
    let internal: Vec<HeaderName> = headers
        .keys()
        .filter(|name| name.as_str().starts_with("x-nextrs-"))
        .cloned()
        .collect();
    for name in internal {
        headers.remove(name);
    }
    // A buffered body's length is computed when it is sent; a streamed one
    // keeps the client's declared length (or goes chunked without one).
    if !streamed {
        headers.remove(header::CONTENT_LENGTH);
    }
    // httpxy: a DELETE or OPTIONS without a body gets `Content-Length: 0`.
    if matches!(*forward.method, Method::DELETE | Method::OPTIONS)
        && !forward.headers.contains_key(header::CONTENT_LENGTH)
        && !forward.headers.contains_key(header::TRANSFER_ENCODING)
    {
        headers.insert(header::CONTENT_LENGTH, HeaderValue::from_static("0"));
    }
    // httpxy's `headers` option: `'x-forwarded-host': req.headers.host || ''`.
    headers.insert(
        HeaderName::from_static("x-forwarded-host"),
        forward
            .client_host
            .cloned()
            .unwrap_or_else(|| HeaderValue::from_static("")),
    );
    headers
}

/// Copies the client's body into a channel the network task reads, chunk by
/// chunk. Past `limit` (or on a client error) it sends an error, so the
/// upstream request is aborted rather than ended early as if complete.
fn pump(body: Body, limit: u64, over_limit: Arc<AtomicBool>) -> UpstreamBody {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<Bytes, std::io::Error>>(1);
    tokio::spawn(async move {
        let mut stream = body.into_data_stream();
        let mut total: u64 = 0;
        while let Some(chunk) = stream.next().await {
            let item = match chunk {
                Ok(chunk) => {
                    total += chunk.len() as u64;
                    if total > limit {
                        over_limit.store(true, Ordering::Release);
                        Err(std::io::Error::other("request body over the limit"))
                    } else {
                        Ok(chunk)
                    }
                }
                Err(error) => Err(std::io::Error::other(error.to_string())),
            };
            let last = item.is_err();
            if tx.send(item).await.is_err() || last {
                return;
            }
        }
    });
    UpstreamBody::Stream(rx)
}

pub(crate) async fn forward(upstream: Option<Upstream>, forward: Forward<'_>) -> Response {
    let Some(upstream) = upstream else {
        return denied(
            "this build rewrites to another origin, and NextRS forwards external rewrites only to destinations its network policy allows (--net-allow)",
        );
    };
    // next start's `new URL()` throws for it and router-server answers
    // with its error response.
    let Ok(url) = url::Url::parse(forward.url) else {
        tracing::warn!(destination = %forward.url, "invalid external rewrite destination");
        return crate::proxy_failed();
    };
    if let Err(denied_by) = upstream.check(&url) {
        tracing::warn!(destination = %url, reason = %denied_by, "external rewrite refused by the network policy");
        return denied(&format!(
            "the external rewrite destination is not allowed by the network policy ({denied_by})"
        ));
    }
    if forward.headers.contains_key(header::UPGRADE) {
        return crate::unsupported(
            "external-rewrite-upgrade",
            "WebSocket upgrades through external rewrites are not supported by NextRS",
        );
    }
    if let Some(length) = forward
        .headers
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        && length > forward.max_body
    {
        return dynamic::too_large(forward.max_body);
    }
    use http_body::Body as _;
    let method = forward.method.clone();
    let over_limit = Arc::new(AtomicBool::new(false));
    let empty = forward.body.is_end_stream();
    // A small body of known size (the proxy's buffered copy, or a short
    // upload) is read whole and sent with its length; anything else streams.
    let small = forward
        .body
        .size_hint()
        .exact()
        .is_some_and(|n| n <= 64 * 1024);
    let streamed = !empty && !small;
    let headers = request_headers(&forward, streamed);
    let max_body = forward.max_body;
    let body = if empty {
        UpstreamBody::Empty
    } else if small {
        match axum::body::to_bytes(
            forward.body,
            usize::try_from(max_body).unwrap_or(usize::MAX),
        )
        .await
        {
            Ok(bytes) => UpstreamBody::Bytes(bytes),
            Err(_) => return dynamic::too_large(max_body),
        }
    } else {
        pump(forward.body, max_body, over_limit.clone())
    };
    let request = UpstreamRequest {
        method,
        url,
        headers,
        body,
    };
    let timeout = forward.timeout;
    let sent = match timeout {
        Some(limit) => match tokio::time::timeout(limit, upstream.send(request)).await {
            Ok(sent) => sent,
            Err(_) => {
                // next start's proxy error handler: 500 Internal Server Error.
                tracing::warn!(
                    timeout_ms = limit.as_millis() as u64,
                    "external rewrite timed out (experimental.proxyTimeout)"
                );
                return proxy_error();
            }
        },
        None => upstream.send(request).await,
    };
    let response = match sent {
        Ok(response) => response,
        Err(error) if error.is_denied() => {
            tracing::warn!(%error, "external rewrite refused by the network policy");
            return denied(&format!(
                "the external rewrite destination is not allowed by the network policy ({error})"
            ));
        }
        Err(error) => {
            if over_limit.load(Ordering::Acquire) {
                return dynamic::too_large(max_body);
            }
            tracing::warn!(code = error.code(), %error, "external rewrite failed");
            return proxy_error();
        }
    };
    let mut headers = response.headers;
    strip_hop_by_hop(&mut headers);
    let chunks = response
        .body
        .map(|chunk| chunk.map_err(|error| std::io::Error::other(error.to_string())));
    // After the head, the same limit applies between parts of the body: an
    // upstream that goes quiet for longer ends the response (next start
    // destroys the proxied response then).
    let body = match timeout {
        Some(limit) => Body::from_stream(futures_util::stream::unfold(
            (Box::pin(chunks), false),
            move |(mut chunks, done)| async move {
                if done {
                    return None;
                }
                match tokio::time::timeout(limit, chunks.next()).await {
                    Ok(Some(item)) => Some((item, (chunks, false))),
                    Ok(None) => None,
                    Err(_) => {
                        tracing::warn!(
                            timeout_ms = limit.as_millis() as u64,
                            "external rewrite body timed out (experimental.proxyTimeout)"
                        );
                        Some((
                            Err(std::io::Error::other(
                                "the upstream went quiet past proxyTimeout",
                            )),
                            (chunks, true),
                        ))
                    }
                }
            },
        )),
        None => Body::from_stream(chunks),
    };
    let mut out = Response::new(body);
    *out.status_mut() = StatusCode::from_u16(response.status).unwrap_or(StatusCode::BAD_GATEWAY);
    *out.headers_mut() = headers;
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hop_by_hop_and_connection_named_headers_are_removed() {
        let mut headers = HeaderMap::new();
        for (name, value) in [
            ("connection", "keep-alive, x-secret-hop"),
            ("keep-alive", "timeout=5"),
            ("x-secret-hop", "1"),
            ("te", "trailers"),
            ("transfer-encoding", "chunked"),
            ("upgrade", "h2c"),
            ("proxy-authorization", "Basic eA=="),
            ("cookie", "a=1"),
            ("authorization", "Bearer t"),
        ] {
            headers.append(
                HeaderName::from_static(name),
                HeaderValue::from_static(value),
            );
        }
        strip_hop_by_hop(&mut headers);
        let left: Vec<&str> = headers.keys().map(HeaderName::as_str).collect();
        assert_eq!(left, ["cookie", "authorization"]);
    }

    #[test]
    fn internal_headers_never_go_upstream_and_forwarded_host_is_the_clients() {
        let mut headers = HeaderMap::new();
        for (name, value) in [
            ("host", "app.example"),
            ("x-middleware-set-cookie", "session=1"),
            ("x-matched-path", "/x"),
            ("x-nextrs-trace", "1"),
            ("x-forwarded-host", "spoofed.example"),
            ("content-length", "4"),
            ("expect", "100-continue"),
            ("accept", "*/*"),
        ] {
            headers.append(
                HeaderName::from_static(name),
                HeaderValue::from_static(value),
            );
        }
        let host = HeaderValue::from_static("app.example");
        let forward = Forward {
            url: "http://upstream.example/x",
            method: &Method::POST,
            headers: &headers,
            client_host: Some(&host),
            body: Body::empty(),
            max_body: 10,
            timeout: None,
        };
        let out = request_headers(&forward, false);
        let mut names: Vec<&str> = out.keys().map(HeaderName::as_str).collect();
        names.sort_unstable();
        assert_eq!(names, ["accept", "x-forwarded-host"]);
        assert_eq!(out["x-forwarded-host"], "app.example");
        let streamed = request_headers(&forward, true);
        assert_eq!(streamed["content-length"], "4");
    }
}
