//! next start's image responses: `sendResponse` / `setResponseHeaders`,
//! `sendEtagResponse` (the `fresh` package) and its error bodies.

use axum::body::Body;
use axum::http::header::{self, HeaderValue};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::Response;
use nextrs_adapter::ImagesConfig;

use super::fetch::Failure;
use super::params::Params;

/// `res.statusCode = status; res.body(message).send()`.
pub(crate) fn text(status: u16, message: &str) -> Response {
    let mut response = Response::new(Body::from(message.to_owned()));
    *response.status_mut() = super::fetch::status_text(status);
    response
}

/// An exception in next start's handler: its router answers 500.
pub(crate) fn internal_error() -> Response {
    text(500, "Internal Server Error")
}

pub(crate) fn failure(failure: &Failure) -> Response {
    match failure {
        Failure::Image(status, message) => text(*status, message),
        Failure::Internal => internal_error(),
        Failure::Overloaded => {
            let mut response = text(503, "Image optimization is overloaded; retry later");
            response
                .headers_mut()
                .insert(header::RETRY_AFTER, HeaderValue::from_static("1"));
            response
        }
    }
}

pub(crate) fn image(
    method: &Method,
    request: &HeaderMap,
    params: &Params,
    entry: &super::Entry,
    x_cache: &'static str,
    images: &ImagesConfig,
) -> Response {
    let mut response = Response::new(Body::empty());
    let headers = response.headers_mut();
    headers.insert(header::VARY, HeaderValue::from_static("Accept"));
    let cache_control = if params.is_static {
        "public, max-age=315360000, immutable".to_owned()
    } else {
        format!("public, max-age={}, must-revalidate", entry.max_age)
    };
    if let Ok(value) = HeaderValue::from_str(&cache_control) {
        headers.insert(header::CACHE_CONTROL, value);
    }
    let etag = HeaderValue::from_str(&entry.etag)
        .ok()
        .filter(|_| !entry.etag.is_empty());
    if let Some(etag) = &etag {
        headers.insert(header::ETAG, etag.clone());
    }
    if fresh(request, etag.as_ref().and_then(|e| e.to_str().ok())) {
        *response.status_mut() = StatusCode::NOT_MODIFIED;
        return response;
    }
    #[allow(unused_mut)]
    let mut content_type = entry.content_type.as_str();
    #[cfg(feature = "negative-controls")]
    {
        content_type = super::controls::content_type(content_type);
    }
    if let Ok(value) = HeaderValue::from_str(content_type) {
        headers.insert(header::CONTENT_TYPE, value);
    }
    let file_name = file_name(&params.href, &entry.content_type);
    let disposition = content_disposition(&file_name, &images.content_disposition_type);
    if let Ok(value) = HeaderValue::from_bytes(&crate::resolve::latin1_bytes(&disposition)) {
        headers.insert(header::CONTENT_DISPOSITION, value);
    }
    if let Ok(value) = HeaderValue::from_str(&images.content_security_policy) {
        headers.insert(header::CONTENT_SECURITY_POLICY, value);
    }
    headers.insert("x-nextjs-cache", HeaderValue::from_static(x_cache));
    headers.insert(header::CONTENT_LENGTH, HeaderValue::from(entry.body.len()));
    if method != Method::HEAD {
        *response.body_mut() = Body::from(entry.body.clone());
    }
    response
}

/// `getFileNameWithExtension(url, contentType)`.
fn file_name(url: &str, content_type: &str) -> String {
    let path = url.split('?').next().unwrap_or("");
    let last = path.rsplit('/').next().unwrap_or("");
    let Some(extension) = nextrs_image::ImageType::from_mime(content_type).map(|t| t.extension())
    else {
        return "image.bin".into();
    };
    if last.is_empty() {
        return "image.bin".into();
    }
    let stem = last.split('.').next().unwrap_or("");
    format!("{stem}.{extension}")
}

/// `contentDisposition(filename, { type })` (content-disposition 0.5).
fn content_disposition(name: &str, kind: &str) -> String {
    // path.basename (POSIX): the text after the last slash.
    let name = name.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    let quoted_ok = !name.is_empty()
        && name
            .chars()
            .all(|c| matches!(c as u32, 0x20..=0x7e | 0x80..=0xff));
    let fallback: String = name
        .chars()
        .flat_map(|c| {
            let keep = matches!(c as u32, 0x20..=0x7e | 0xa0..=0xff);
            let units = if keep { 1 } else { c.len_utf16() };
            std::iter::repeat_n(if keep { c } else { '?' }, units)
        })
        .collect();
    let has_fallback = fallback != name;
    let hex_escape = name
        .as_bytes()
        .windows(3)
        .any(|w| w[0] == b'%' && w[1].is_ascii_hexdigit() && w[2].is_ascii_hexdigit());
    let mut out = kind.to_ascii_lowercase();
    if quoted_ok || has_fallback {
        let value = if has_fallback { &fallback } else { name };
        let escaped: String = value
            .chars()
            .flat_map(|c| match c {
                '\\' | '"' => vec!['\\', c],
                c => vec![c],
            })
            .collect();
        out.push_str(&format!("; filename=\"{escaped}\""));
    }
    if has_fallback || !quoted_ok || hex_escape {
        out.push_str("; filename*=UTF-8''");
        out.push_str(&ext_value(name));
    }
    out
}

/// `encodeURIComponent(value)` with `'()*` (and the rest of RFC 5987's
/// specials) percent-encoded too.
fn ext_value(value: &str) -> String {
    let mut out = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'!' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// The `fresh` package, without `Last-Modified` (image responses have none).
fn fresh(request: &HeaderMap, etag: Option<&str>) -> bool {
    let get = |name: header::HeaderName| {
        request
            .get(name)
            .map(|v| crate::resolve::latin1(v.as_bytes()))
    };
    let modified_since = get(header::IF_MODIFIED_SINCE).filter(|v| !v.is_empty());
    let none_match = get(header::IF_NONE_MATCH).filter(|v| !v.is_empty());
    if modified_since.is_none() && none_match.is_none() {
        return false;
    }
    if let Some(cache_control) = get(header::CACHE_CONTROL)
        && no_cache(&cache_control)
    {
        return false;
    }
    if let Some(none_match) = &none_match
        && none_match != "*"
    {
        let Some(etag) = etag else {
            return false;
        };
        let matched = none_match
            .split([',', ' '])
            .filter(|t| !t.is_empty())
            .any(|t| t == etag || t == format!("W/{etag}") || format!("W/{t}") == etag);
        if !matched {
            return false;
        }
    }
    // With If-Modified-Since and no Last-Modified, the response is stale.
    modified_since.is_none()
}

/// `/(?:^|,)\s*?no-cache\s*?(?:,|$)/`
fn no_cache(value: &str) -> bool {
    value.split(',').any(|d| d.trim() == "no-cache")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Expected values: content-disposition as vendored by Next 16.4.3.
    #[test]
    fn disposition_like_next() {
        assert_eq!(
            content_disposition("photo.webp", "attachment"),
            "attachment; filename=\"photo.webp\""
        );
        assert_eq!(
            content_disposition("日本.webp", "attachment"),
            "attachment; filename=\"??.webp\"; filename*=UTF-8''%E6%97%A5%E6%9C%AC.webp"
        );
        assert_eq!(
            content_disposition("my%20photo.webp", "attachment"),
            "attachment; filename=\"my%20photo.webp\"; filename*=UTF-8''my%2520photo.webp"
        );
        assert_eq!(
            content_disposition("a;b.webp", "attachment"),
            "attachment; filename=\"a;b.webp\""
        );
        assert_eq!(
            content_disposition("fötö.webp", "inline"),
            "inline; filename=\"fötö.webp\""
        );
        assert_eq!(
            content_disposition("a b\"c.png", "attachment"),
            "attachment; filename=\"a b\\\"c.png\""
        );
    }

    #[test]
    fn file_names() {
        assert_eq!(file_name("/photos/a.jpg", "image/webp"), "a.webp");
        assert_eq!(file_name("/photos/a.b.jpg?x=1", "image/jpeg"), "a.jpeg");
        assert_eq!(file_name("https://h/x/", "image/webp"), "image.bin");
        assert_eq!(file_name("/photos/.hidden", "image/png"), ".png");
    }

    #[test]
    fn conditional_requests() {
        let mut h = HeaderMap::new();
        assert!(!fresh(&h, Some("abc")));
        h.insert(header::IF_NONE_MATCH, HeaderValue::from_static("abc"));
        assert!(fresh(&h, Some("abc")));
        assert!(!fresh(&h, Some("abd")));
        h.insert(header::IF_NONE_MATCH, HeaderValue::from_static("W/abc, x"));
        assert!(fresh(&h, Some("abc")));
        h.insert(header::IF_NONE_MATCH, HeaderValue::from_static("*"));
        assert!(fresh(&h, None));
        h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
        assert!(!fresh(&h, Some("abc")));
        let mut h = HeaderMap::new();
        h.insert(
            header::IF_MODIFIED_SINCE,
            HeaderValue::from_static("Sat, 01 Jan 2000 00:00:00 GMT"),
        );
        assert!(!fresh(&h, Some("abc")));
    }
}
