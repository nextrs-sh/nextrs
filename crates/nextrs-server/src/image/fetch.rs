//! The source image: `fetchInternalImage` (the request goes through the
//! server's own routing, as next start sends it to its router) and
//! `fetchExternalImage` (under the NextRS network policy).

use std::time::Duration;

use axum::body::Body;
use axum::http::{HeaderMap, StatusCode, header};
use bytes::{Bytes, BytesMut};
use futures_util::StreamExt;

/// Next's agent timeout for one upstream request (`agentOptions.timeout`).
const EXTERNAL_TIMEOUT: Duration = Duration::from_secs(7);

#[derive(Debug, Clone)]
pub(crate) struct Source {
    pub buffer: Bytes,
    pub cache_control: Option<String>,
    /// `extractEtag`: the upstream's ETag base64url-encoded, else a hash.
    pub etag: String,
}

/// Why no source image was obtained.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Failure {
    /// next start's `ImageError`: this status (400 or above, else 500)
    /// with this body.
    Image(u16, &'static str),
    /// next start throws a plain error: 500 from its router.
    Internal,
    /// NextRS's own limit on waiting transformations.
    Overloaded,
}

impl Failure {
    fn image(status: u16, message: &'static str) -> Self {
        Self::Image(if status >= 400 { status } else { 500 }, message)
    }
}

const INTERNAL_INVALID: &str = "\"url\" parameter is valid but internal response is invalid";
const UPSTREAM_INVALID: &str = "\"url\" parameter is valid but upstream response is invalid";
pub(crate) const NOT_ALLOWED: &str = "\"url\" parameter is not allowed";
const TIMED_OUT: &str = "\"url\" parameter is valid but upstream response timed out";

fn header_string(headers: &HeaderMap, name: header::HeaderName) -> Option<String> {
    headers
        .get(name)
        .map(|v| crate::resolve::latin1(v.as_bytes()))
}

/// An internal response (`MockedResponse`): 2xx with a body below the limit.
pub(crate) async fn internal(
    response: axum::response::Response,
    max_body: u64,
) -> Result<Source, Failure> {
    let status = response.status();
    if !status.is_success() {
        tracing::error!(status = status.as_u16(), "internal image response failed");
        return Err(Failure::image(status.as_u16(), INTERNAL_INVALID));
    }
    let headers = response.headers().clone();
    let mut body = response.into_body().into_data_stream();
    let mut buffer = BytesMut::new();
    while let Some(chunk) = body.next().await {
        let Ok(chunk) = chunk else {
            return Err(Failure::image(500, UPSTREAM_INVALID));
        };
        if (buffer.len() + chunk.len()) as u64 > max_body {
            return Err(Failure::image(413, INTERNAL_INVALID));
        }
        buffer.extend_from_slice(&chunk);
    }
    if buffer.is_empty() {
        return Err(Failure::image(400, INTERNAL_INVALID));
    }
    let buffer = buffer.freeze();
    Ok(Source {
        etag: extract_etag(header_string(&headers, header::ETAG).as_deref(), &buffer),
        cache_control: header_string(&headers, header::CACHE_CONTROL),
        buffer,
    })
}

fn is_redirect(status: u16) -> bool {
    matches!(status, 301 | 302 | 303 | 307 | 308)
}

/// `fetchExternalImage(href, .., maximumResponseBody, maximumRedirects)`.
/// Every hop is checked by the network policy before connecting and by
/// the resolver for every address it connects to.
pub(crate) async fn external(
    upstream: Option<&nextrs_runtime::Upstream>,
    href: &str,
    max_body: u64,
    max_redirects: u32,
) -> Result<Source, Failure> {
    let Some(upstream) = upstream else {
        tracing::warn!(
            origin = %origin(href),
            "remote image refused: no network policy (--net-allow) is configured"
        );
        return Err(Failure::Image(400, NOT_ALLOWED));
    };
    let mut url: url::Url = href.parse().map_err(|_| Failure::Internal)?;
    let mut remaining = max_redirects;
    loop {
        match tokio::time::timeout(EXTERNAL_TIMEOUT, hop(upstream, &url, max_body)).await {
            Err(_) => {
                tracing::error!(origin = %origin(url.as_str()), "upstream image response timed out");
                return Err(Failure::Image(504, TIMED_OUT));
            }
            Ok(Hop::Done(result)) => return result,
            Ok(Hop::Redirect(next)) => {
                if remaining == 0 {
                    tracing::error!(origin = %origin(url.as_str()), "upstream image response had too many redirects");
                    return Err(Failure::Image(508, UPSTREAM_INVALID));
                }
                remaining -= 1;
                url = next;
            }
        }
    }
}

enum Hop {
    Done(Result<Source, Failure>),
    Redirect(url::Url),
}

fn origin(href: &str) -> String {
    url::Url::parse(href)
        .map(|u| u.origin().ascii_serialization())
        .unwrap_or_default()
}

async fn hop(upstream: &nextrs_runtime::Upstream, url: &url::Url, max_body: u64) -> Hop {
    let mut headers = reqwest_headers();
    headers.remove("host");
    let request = nextrs_runtime::UpstreamRequest {
        method: reqwest_get(),
        url: url.clone(),
        headers,
        body: nextrs_runtime::UpstreamBody::Empty,
        decompress: true,
    };
    let response = match upstream.send(request).await {
        Ok(response) => response,
        Err(error) if error.is_denied() => {
            tracing::warn!(origin = %url.origin().ascii_serialization(), %error, "remote image refused by the network policy");
            return Hop::Done(Err(Failure::Image(400, NOT_ALLOWED)));
        }
        Err(error) => {
            tracing::error!(origin = %url.origin().ascii_serialization(), %error, "upstream image request failed");
            return Hop::Done(Err(Failure::Internal));
        }
    };
    let status = response.status;
    let location = response
        .headers
        .get("location")
        .and_then(|v| v.to_str().ok())
        .and_then(|l| url.join(l).ok());
    if is_redirect(status)
        && let Some(next) = location
    {
        // The redirect body is dropped unread (the stream aborts it).
        return Hop::Redirect(next);
    }
    if !(200..=299).contains(&status) {
        tracing::error!(origin = %url.origin().ascii_serialization(), status, "upstream image response failed");
        return Hop::Done(Err(Failure::image(status, UPSTREAM_INVALID)));
    }
    let mut body = response.body;
    let mut buffer = BytesMut::new();
    while let Some(chunk) = body.next().await {
        let Ok(chunk) = chunk else {
            return Hop::Done(Err(Failure::Internal));
        };
        if (buffer.len() + chunk.len()) as u64 > max_body {
            tracing::error!(origin = %url.origin().ascii_serialization(), "upstream image response exceeded maximum size");
            return Hop::Done(Err(Failure::Image(413, UPSTREAM_INVALID)));
        }
        buffer.extend_from_slice(&chunk);
    }
    if buffer.is_empty() {
        return Hop::Done(Err(Failure::Image(400, UPSTREAM_INVALID)));
    }
    let buffer = buffer.freeze();
    let header = |name: &str| {
        response
            .headers
            .get(name)
            .map(|v| crate::resolve::latin1(v.as_bytes()))
    };
    Hop::Done(Ok(Source {
        etag: extract_etag(header("etag").as_deref(), &buffer),
        cache_control: header("cache-control"),
        buffer,
    }))
}

fn reqwest_get() -> reqwest_method::Method {
    reqwest_method::Method::GET
}

/// Next's `BASE_REQ_HEADERS` (the HTTP client adds `Accept-Encoding`).
fn reqwest_headers() -> reqwest_method::HeaderMap {
    let mut headers = reqwest_method::HeaderMap::new();
    for (name, value) in [
        ("accept", "*/*"),
        ("accept-language", "*"),
        ("sec-fetch-mode", "cors"),
        ("user-agent", "node"),
    ] {
        headers.insert(name, reqwest_method::HeaderValue::from_static(value));
    }
    headers
}

/// The HTTP types `nextrs_runtime::UpstreamRequest` uses (reqwest's
/// re-exports of `http`, the same crate axum uses).
mod reqwest_method {
    pub use axum::http::{HeaderMap, HeaderValue, Method};
}

/// `extractEtag(etag, buffer)`.
pub(crate) fn extract_etag(etag: Option<&str>, buffer: &[u8]) -> String {
    use base64::Engine;
    match etag {
        Some(etag) if !etag.is_empty() => {
            // `Buffer.from(etag)` encodes the header string as UTF-8.
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(etag.as_bytes())
        }
        _ => image_etag(buffer),
    }
}

/// `getImageEtag(buffer)`.
pub(crate) fn image_etag(buffer: &[u8]) -> String {
    use base64::Engine;
    use sha2::Digest;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha2::Sha256::digest(buffer))
}

/// `getMaxAge(cacheControl)`: `s-maxage`, else `max-age`, else 0.
pub(crate) fn max_age(cache_control: Option<&str>) -> u64 {
    let Some(value) = cache_control else {
        return 0;
    };
    let mut map: Vec<(String, String)> = Vec::new();
    for directive in value.split(',') {
        // `split('=', 2)`: a second `=` and what follows are dropped.
        let mut parts = directive.trim().split('=');
        let key = parts.next().unwrap_or("");
        let val = parts.next().unwrap_or("");
        map.push((key.to_lowercase(), val.to_owned()));
    }
    // A Map: the last occurrence of a directive wins; an empty value is falsy.
    let get = |k: &str| {
        map.iter()
            .rev()
            .find(|(key, _)| key == k)
            .map(|(_, v)| v.clone())
            .filter(|v| !v.is_empty())
    };
    let mut age = get("s-maxage")
        .or_else(|| get("max-age"))
        .unwrap_or_default();
    if age.len() >= 2 && age.starts_with('"') && age.ends_with('"') {
        age = age[1..age.len() - 1].to_owned();
    }
    // parseInt: leading digits.
    let digits: String = age
        .trim_start()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().unwrap_or(0)
}

/// The request next start's `fetchInternalImage` makes: GET, no headers.
pub(crate) fn internal_request(href: &str) -> Option<axum::http::Request<Body>> {
    axum::http::Request::builder()
        .method("GET")
        .uri(href)
        .body(Body::empty())
        .ok()
}

pub(crate) fn status_text(status: u16) -> StatusCode {
    StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_age_like_next() {
        assert_eq!(max_age(None), 0);
        assert_eq!(max_age(Some("public, max-age=60")), 60);
        assert_eq!(max_age(Some("max-age=60, s-maxage=120")), 120);
        assert_eq!(max_age(Some("max-age=\"30\"")), 30);
        assert_eq!(max_age(Some("no-store")), 0);
        assert_eq!(max_age(Some("max-age=abc")), 0);
    }

    #[test]
    fn etags_like_next() {
        // Buffer.from('W/"5-abc"').toString('base64url')
        assert_eq!(extract_etag(Some("W/\"5-abc\""), b""), "Vy8iNS1hYmMi");
        // getHash([Buffer.from('abc')])
        assert_eq!(
            extract_etag(None, b"abc"),
            "ungWv48Bz-pBQUDeXa4iI7ADYaOWF3qctBD_YfIAFa0"
        );
    }
}
