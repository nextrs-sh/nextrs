//! Response compression, as next start's router-server applies it: the
//! `compression` 1.7 middleware with its defaults, on every response unless
//! next.config sets `compress: false` (`next/dist/compiled/compression`).
//!
//! The decision follows the middleware step by step:
//!
//! 1. only a compressible `Content-Type` (the `compressible` package:
//!    mime-db's compressible types, `text/*`, `*+json`, `*+text`, `*+xml`);
//! 2. not with `Cache-Control: no-transform`;
//! 3. `Vary: Accept-Encoding` is added from here on, compressed or not;
//! 4. not below 1 KiB when the size is known (a streamed body of unknown
//!    size is compressed whatever its size);
//! 5. not when the response already has a `Content-Encoding`;
//! 6. not for HEAD;
//! 7. the encoding `negotiator` picks from `gzip, deflate, identity` for the
//!    request's `Accept-Encoding` (deflate only when gzip is not
//!    acceptable).
//!
//! Where NextRS differs, on purpose:
//! - gzip only: a client that accepts deflate but not gzip (none of the
//!   browsers) gets the identity response, where next start sends deflate;
//! - a partial response (206, or any `Content-Range`) is never compressed
//!   (the middleware compresses the partial body, so its Content-Range no
//!   longer describes the bytes sent);
//! - a strong ETag becomes weak on the compressed response (`W/"…"`), as
//!   nginx does: the gzip bytes are a different representation, which a
//!   strong validator must not claim to be identical (next start keeps it);
//!   `If-None-Match` compares weakly, so revalidation still gets its 304;
//! - `Accept-Ranges` is removed from a compressed response (ranges apply to
//!   the identity bytes).
//!
//! Streaming: every chunk the route writes is compressed and flushed with a
//! zlib sync flush before the next one is read, as Next's pipe to the
//! response calls the middleware's `res.flush()` after each chunk: HTML,
//! RSC and Partial Prerendering streams keep their progression. Nothing is
//! buffered beyond the chunk in hand (a large chunk is fed to zlib in
//! slices), the inner body is only polled when the client takes the
//! previous output (backpressure), and dropping the response (a client
//! that went away) drops the route's body as before.

use std::io::Write;
use std::pin::Pin;
use std::task::{Context, Poll, ready};

use axum::body::{Body, Bytes};
use axum::http::header::{self, HeaderMap, HeaderValue};
use axum::http::{Method, StatusCode, Version};
use axum::response::Response;
use flate2::Compression;
use flate2::write::GzEncoder;
use http_body::{Body as _, Frame, SizeHint};

/// The middleware's default `threshold` (`bytes.parse('1kb')`).
const THRESHOLD: u64 = 1024;
/// How much of a chunk is handed to zlib at a time: bounds the work done in
/// one poll and the size of one output frame.
const SLICE: usize = 32 * 1024;

/// mime-db types marked compressible that the `compressible` package's
/// pattern does not already cover (from the mime-db that Next.js 16.4.6
/// bundles; no type it marks uncompressible matches the pattern). Sorted.
const COMPRESSIBLE_TYPES: &[&str] = &[
    "application/dart",
    "application/ecmascript",
    "application/javascript",
    "application/json",
    "application/postscript",
    "application/raml+yaml",
    "application/rtf",
    "application/tar",
    "application/toml",
    "application/vnd.dart",
    "application/vnd.ms-fontobject",
    "application/vnd.ms-opentype",
    "application/wasm",
    "application/x-httpd-php",
    "application/x-javascript",
    "application/x-ns-proxy-autoconfig",
    "application/x-sh",
    "application/x-tar",
    "application/x-virtualbox-hdd",
    "application/x-virtualbox-ova",
    "application/x-virtualbox-ovf",
    "application/x-virtualbox-vbox",
    "application/x-virtualbox-vdi",
    "application/x-virtualbox-vhd",
    "application/x-virtualbox-vmdk",
    "application/x-www-form-urlencoded",
    "application/xml",
    "application/xml-dtd",
    "font/otf",
    "font/ttf",
    "image/bmp",
    "image/vnd.adobe.photoshop",
    "image/x-icon",
    "image/x-ms-bmp",
    "message/rfc822",
    "model/gltf-binary",
    "x-shader/x-fragment",
    "x-shader/x-vertex",
];

/// The `compressible` package: the media type (before parameters or
/// whitespace, lowercased) is in mime-db as compressible, or matches
/// `^text\/|\+(?:json|text|xml)$`.
pub(crate) fn compressible(content_type: &str) -> bool {
    let media = content_type
        .trim_start()
        .split([';', ' ', '\t', '\r', '\n'])
        .next()
        .unwrap_or_default();
    if media.is_empty() {
        return false;
    }
    // Lowercased only when needed: every response passes here.
    let media: std::borrow::Cow<'_, str> = if media.bytes().any(|b| b.is_ascii_uppercase()) {
        media.to_ascii_lowercase().into()
    } else {
        media.into()
    };
    COMPRESSIBLE_TYPES.binary_search(&media.as_ref()).is_ok()
        || media.starts_with("text/")
        || media.ends_with("+json")
        || media.ends_with("+text")
        || media.ends_with("+xml")
}

/// One `Accept-Encoding` entry as `negotiator` parses it.
struct Accepted<'a> {
    encoding: &'a str,
    q: f64,
    index: usize,
}

/// JavaScript's `parseFloat`: the longest numeric prefix, else NaN.
fn parse_float(text: &str) -> f64 {
    let text = text.trim_start();
    let bytes = text.as_bytes();
    let mut end = 0;
    let mut seen_digit = false;
    let mut seen_dot = false;
    if matches!(bytes.first(), Some(b'+' | b'-')) {
        end = 1;
    }
    while let Some(&b) = bytes.get(end) {
        match b {
            b'0'..=b'9' => seen_digit = true,
            b'.' if !seen_dot => seen_dot = true,
            _ => break,
        }
        end += 1;
    }
    if !seen_digit {
        return if text[end..].starts_with("Infinity") {
            if text.starts_with('-') {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            }
        } else {
            f64::NAN
        };
    }
    // An exponent, when complete.
    let mut exp_end = end;
    if matches!(bytes.get(exp_end), Some(b'e' | b'E')) {
        exp_end += 1;
        if matches!(bytes.get(exp_end), Some(b'+' | b'-')) {
            exp_end += 1;
        }
        let digits = bytes[exp_end..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if digits > 0 {
            end = exp_end + digits;
        }
    }
    text[..end].parse().unwrap_or(f64::NAN)
}

/// `parseAcceptEncoding`: entries in order, plus an implicit `identity`
/// with the lowest quality given (1 when none) unless one is listed (or
/// `*`).
fn parse_accept_encoding(header: &str) -> Vec<Accepted<'_>> {
    let mut out = Vec::new();
    let mut has_identity = false;
    let mut min_q: f64 = 1.0;
    let parts: Vec<&str> = header.split(',').collect();
    for (index, part) in parts.iter().enumerate() {
        // /^\s*([^\s;]+)\s*(?:;(.*))?$/
        let part = part.trim();
        let (name, params) = match part.find(';') {
            Some(at) => (part[..at].trim_end(), Some(&part[at + 1..])),
            None => (part, None),
        };
        if name.is_empty() || name.contains(char::is_whitespace) {
            continue;
        }
        let mut q = 1.0;
        if let Some(params) = params {
            for param in params.split(';') {
                let mut kv = param.trim().split('=');
                if kv.next() == Some("q") {
                    q = parse_float(kv.next().unwrap_or_default());
                    break;
                }
            }
        }
        has_identity |= name.eq_ignore_ascii_case("identity") || name == "*";
        // `Math.min(n, c.q || 1)`: a quality of 0 or NaN counts as 1.
        let effective = if q == 0.0 || q.is_nan() { 1.0 } else { q };
        min_q = min_q.min(effective);
        out.push(Accepted {
            encoding: name,
            q,
            index,
        });
    }
    if !has_identity {
        out.push(Accepted {
            encoding: "identity",
            q: min_q,
            index: parts.len(),
        });
    }
    out
}

/// `preferredEncodings(header, available)[0]`.
fn preferred<'a>(accepted: &[Accepted<'_>], available: &[&'a str]) -> Option<&'a str> {
    // For each available encoding, its best match: specificity (an exact
    // name over `*`), then quality, then the earlier entry.
    struct Priority {
        q: f64,
        s: u8,
        o: isize,
        i: usize,
    }
    let mut priorities: Vec<Priority> = Vec::new();
    for (i, name) in available.iter().enumerate() {
        let mut best = Priority {
            q: 0.0,
            s: 0,
            o: -1,
            i,
        };
        for entry in accepted {
            let s = if entry.encoding.eq_ignore_ascii_case(name) {
                1
            } else if entry.encoding == "*" {
                0
            } else {
                continue;
            };
            let o = entry.index as isize;
            // `(n.s - o.s || n.q - o.q || n.o - o.o) < 0`
            let replace = match best.s.cmp(&s) {
                std::cmp::Ordering::Less => true,
                std::cmp::Ordering::Greater => false,
                std::cmp::Ordering::Equal => {
                    let dq = best.q - entry.q;
                    if dq != 0.0 && !dq.is_nan() {
                        dq < 0.0
                    } else {
                        best.o < o
                    }
                }
            };
            if replace {
                best = Priority {
                    q: entry.q,
                    s,
                    o,
                    i,
                };
            }
        }
        priorities.push(best);
    }
    // `filter(q > 0).sort(compareSpecs)`: quality, specificity, entry
    // order, then the order of `available`.
    priorities.retain(|p| p.q > 0.0);
    priorities.sort_by(|a, b| {
        b.q.partial_cmp(&a.q)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.s.cmp(&a.s))
            .then(a.o.cmp(&b.o))
            .then(a.i.cmp(&b.i))
    });
    priorities.first().map(|p| available[p.i])
}

/// The middleware's choice for this `Accept-Encoding` (Node joins repeated
/// headers with ", "; an absent header is ""), reduced to what NextRS
/// sends: gzip when it chose gzip, otherwise nothing.
pub(crate) fn wants_gzip(request: &HeaderMap) -> bool {
    let mut values = request.get_all(header::ACCEPT_ENCODING).iter();
    let header: std::borrow::Cow<'_, str> = match (values.next(), values.next()) {
        // No header: identity only (negotiator's "" input).
        (None, _) => return false,
        (Some(one), None) => String::from_utf8_lossy(one.as_bytes()),
        _ => request
            .get_all(header::ACCEPT_ENCODING)
            .iter()
            .map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned())
            .collect::<Vec<_>>()
            .join(", ")
            .into(),
    };
    // Without qualities or `*` (the browsers' values) every entry has
    // q=1, and negotiator prefers the coding listed first, a repeated one
    // counting at its last position: gzip wins when it comes before an
    // explicit identity (deflate only ever defers to gzip). Same answer as
    // the full negotiation, no allocation.
    if !header.contains(['q', 'Q', '*', ';']) {
        let (mut gzip, mut identity) = (None, None);
        for (i, name) in header.split(',').map(str::trim).enumerate() {
            if name.eq_ignore_ascii_case("gzip") {
                gzip = Some(i);
            } else if name.eq_ignore_ascii_case("identity") {
                identity = Some(i);
            }
        }
        return match (gzip, identity) {
            (Some(g), Some(i)) => g < i,
            (Some(_), None) => true,
            (None, _) => false,
        };
    }
    let accepted = parse_accept_encoding(&header);
    let mut choice = preferred(&accepted, &["gzip", "deflate", "identity"]);
    if choice == Some("deflate") && preferred(&accepted, &["gzip"]).is_some() {
        choice = preferred(&accepted, &["gzip", "identity"]);
    }
    choice == Some("gzip")
}

/// The `vary` package: add `Accept-Encoding` unless the field list has it
/// or is `*`.
fn add_vary(headers: &mut HeaderMap) {
    // One Vary value (or none), as Next writes it: no allocation unless the
    // header changes.
    let mut values = headers.get_all(header::VARY).iter();
    if let (first, None) = (values.next(), values.next()) {
        let current = first.map_or("", |v| v.to_str().unwrap_or(""));
        if !current.is_empty() || first.is_none() {
            let present = current
                .split(',')
                .map(str::trim)
                .any(|f| f == "*" || f.eq_ignore_ascii_case("accept-encoding"));
            if present {
                return;
            }
            let value = if current.trim().is_empty() {
                HeaderValue::from_static("Accept-Encoding")
            } else {
                match extended_vary(first.expect("non-empty")) {
                    Some(value) => value,
                    None => return,
                }
            };
            headers.insert(header::VARY, value);
            return;
        }
    }
    let existing: Vec<String> = headers
        .get_all(header::VARY)
        .iter()
        .map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned())
        .collect();
    let joined = existing.join(", ");
    let fields: Vec<&str> = joined
        .split(',')
        .map(str::trim)
        .filter(|f| !f.is_empty())
        .collect();
    if fields
        .iter()
        .any(|f| *f == "*" || f.eq_ignore_ascii_case("accept-encoding"))
    {
        return;
    }
    let value = if joined.trim().is_empty() {
        "Accept-Encoding".to_owned()
    } else {
        format!("{joined}, Accept-Encoding")
    };
    if let Ok(value) = HeaderValue::from_str(&value) {
        headers.insert(header::VARY, value);
    }
}

/// `<vary>, Accept-Encoding`. A build has a handful of distinct Vary
/// values (Next's RSC fields on every page), so the last one is kept per
/// thread and reused (a HeaderValue clone shares its bytes).
fn extended_vary(current: &HeaderValue) -> Option<HeaderValue> {
    thread_local! {
        static LAST: std::cell::RefCell<Option<(HeaderValue, HeaderValue)>> =
            const { std::cell::RefCell::new(None) };
    }
    LAST.with(|last| {
        let mut last = last.borrow_mut();
        if let Some((input, output)) = last.as_ref()
            && input == current
        {
            return Some(output.clone());
        }
        let mut bytes = current.as_bytes().to_vec();
        bytes.extend_from_slice(b", Accept-Encoding");
        let output = HeaderValue::from_bytes(&bytes).ok()?;
        *last = Some((current.clone(), output.clone()));
        Some(output)
    })
}

fn no_transform(headers: &HeaderMap) -> bool {
    // /(?:^|,)\s*?no-transform\s*?(?:,|$)/ on the (joined) header.
    headers.get_all(header::CACHE_CONTROL).iter().any(|v| {
        String::from_utf8_lossy(v.as_bytes())
            .split(',')
            .any(|d| d.trim() == "no-transform")
    })
}

/// What the request contributes to the decision.
#[derive(Clone, Copy)]
pub(crate) struct Negotiated {
    gzip: bool,
    head: bool,
    version: Version,
}

impl Negotiated {
    pub(crate) fn of(method: &Method, version: Version, headers: &HeaderMap) -> Self {
        Self {
            gzip: wants_gzip(headers),
            head: method == Method::HEAD,
            version,
        }
    }

    /// The response as the middleware would send it.
    pub(crate) fn apply(self, mut response: Response) -> Response {
        let headers = response.headers();
        let Some(content_type) = headers.get(header::CONTENT_TYPE) else {
            return response;
        };
        if !compressible(&String::from_utf8_lossy(content_type.as_bytes())) || no_transform(headers)
        {
            return response;
        }
        add_vary(response.headers_mut());
        let headers = response.headers();
        let status = response.status();
        let declared = headers
            .get(header::CONTENT_LENGTH)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok());
        let small = declared.is_some_and(|n| n < THRESHOLD)
            || response
                .body()
                .size_hint()
                .exact()
                .is_some_and(|n| n < THRESHOLD);
        let encoded = headers
            .get(header::CONTENT_ENCODING)
            .is_some_and(|v| v.as_bytes() != b"identity");
        let partial =
            status == StatusCode::PARTIAL_CONTENT || headers.contains_key(header::CONTENT_RANGE);
        let bodyless = status.is_informational()
            || status == StatusCode::NO_CONTENT
            || status == StatusCode::NOT_MODIFIED;
        if small || encoded || self.head || partial || bodyless || !self.gzip {
            return response;
        }
        let headers = response.headers_mut();
        headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("gzip"));
        headers.remove(header::CONTENT_LENGTH);
        headers.remove(header::ACCEPT_RANGES);
        if let Some(etag) = headers.get(header::ETAG)
            && !etag.as_bytes().starts_with(b"W/")
        {
            let mut weak = b"W/".to_vec();
            weak.extend_from_slice(etag.as_bytes());
            if let Ok(weak) = HeaderValue::from_bytes(&weak) {
                headers.insert(header::ETAG, weak);
            }
        }
        let response = response.map(|body| Body::new(GzipBody::new(body)));
        // Without a length, an HTTP/1.0 response is delimited by closing
        // the connection.
        crate::dynamic::frame_for(self.version, response)
    }
}

/// A gzip stream of the inner body: each data frame is compressed and
/// sync-flushed into one or more output frames before the next is read.
pub(crate) struct GzipBody {
    inner: Body,
    encoder: Option<GzEncoder<Vec<u8>>>,
    /// The rest of the current input frame.
    pending: Bytes,
    /// Trailers from the inner body, sent after the gzip trailer.
    trailers: Option<HeaderMap>,
    done: bool,
}

impl GzipBody {
    pub(crate) fn new(inner: Body) -> Self {
        Self {
            inner,
            encoder: Some(GzEncoder::new(Vec::new(), Compression::default())),
            pending: Bytes::new(),
            trailers: None,
            done: false,
        }
    }

    fn take_output(&mut self) -> Option<Bytes> {
        let encoder = self.encoder.as_mut()?;
        let out = std::mem::take(encoder.get_mut());
        (!out.is_empty()).then(|| Bytes::from(out))
    }
}

fn io_error(error: std::io::Error) -> axum::Error {
    axum::Error::new(error)
}

impl http_body::Body for GzipBody {
    type Data = Bytes;
    type Error = axum::Error;

    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Self::Data>, Self::Error>>> {
        let this = &mut *self;
        loop {
            if this.done {
                return Poll::Ready(this.trailers.take().map(|t| Ok(Frame::trailers(t))));
            }
            // Feed the chunk in hand, a slice at a time; flush at its end.
            if !this.pending.is_empty() {
                let take = this.pending.len().min(SLICE);
                let slice = this.pending.split_to(take);
                let last = this.pending.is_empty();
                let encoder = this.encoder.as_mut().expect("encoder until done");
                let written = encoder
                    .write_all(&slice)
                    .and_then(|()| if last { encoder.flush() } else { Ok(()) });
                if let Err(error) = written {
                    this.done = true;
                    return Poll::Ready(Some(Err(io_error(error))));
                }
                if let Some(out) = this.take_output() {
                    return Poll::Ready(Some(Ok(Frame::data(out))));
                }
                continue;
            }
            match ready!(Pin::new(&mut this.inner).poll_frame(cx)) {
                Some(Ok(frame)) => match frame.into_data() {
                    Ok(data) => this.pending = data,
                    Err(frame) => {
                        if let Ok(trailers) = frame.into_trailers() {
                            this.trailers = Some(trailers);
                        }
                    }
                },
                Some(Err(error)) => {
                    // The route's stream failed: end without the gzip
                    // trailer, so the client sees a truncated stream (as it
                    // would see a truncated identity body).
                    this.done = true;
                    this.encoder = None;
                    return Poll::Ready(Some(Err(error)));
                }
                None => {
                    this.done = true;
                    let encoder = this.encoder.take().expect("encoder until done");
                    match encoder.finish() {
                        Ok(out) if !out.is_empty() => {
                            return Poll::Ready(Some(Ok(Frame::data(Bytes::from(out)))));
                        }
                        Ok(_) => {}
                        Err(error) => return Poll::Ready(Some(Err(io_error(error)))),
                    }
                }
            }
        }
    }

    fn is_end_stream(&self) -> bool {
        self.done && self.trailers.is_none()
    }

    fn size_hint(&self) -> SizeHint {
        SizeHint::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn accepts(value: &str) -> bool {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::ACCEPT_ENCODING,
            HeaderValue::from_str(value).unwrap(),
        );
        wants_gzip(&headers)
    }

    /// next start 16.4.6 (`compression` 1.7.5 / `negotiator` 0.6.4), each
    /// value sent to it: gzip when it answered gzip.
    #[test]
    fn negotiation_follows_negotiator() {
        assert!(!wants_gzip(&HeaderMap::new()));
        for (value, gzip) in [
            ("gzip", true),
            ("GZIP", true),
            ("gzip, deflate, br, zstd", true),
            ("br, gzip", true),
            ("br", false),
            ("deflate", false),
            ("identity", false),
            ("", false),
            ("*", true),
            ("gzip;q=0", false),
            ("gzip;q=0, *", false),
            ("*;q=0", false),
            ("gzip;q=0.5", true),
            ("gzip;q=0.5, identity", false),
            ("identity, gzip", false),
            ("gzip, identity", true),
            ("deflate, gzip;q=0.5", true),
            ("deflate;q=1, gzip;q=0.9, identity;q=0.95", false),
            ("gzip;q=abc", false),
            ("x-gzip", false),
            (" gzip ; q=1 ", true),
            ("gzip;level=1;q=0.2", true),
            (",,,", false),
            ("identity;q=0", false),
            ("identity;q=0, gzip", true),
        ] {
            assert_eq!(accepts(value), gzip, "{value:?}");
        }
    }

    /// The fast path gives the full negotiation's answer for every list
    /// of plain codings.
    #[test]
    fn fast_path_agrees_with_negotiator() {
        let names = [
            "gzip", "GZip", "deflate", "identity", "br", "zstd", "x-gzip", "",
        ];
        let mut lists = vec![String::new()];
        for a in names {
            lists.push(a.to_owned());
            for b in names {
                lists.push(format!("{a}, {b}"));
                for c in names {
                    lists.push(format!("{a},{b} , {c}"));
                }
            }
        }
        for list in lists {
            let accepted = parse_accept_encoding(&list);
            let mut choice = preferred(&accepted, &["gzip", "deflate", "identity"]);
            if choice == Some("deflate") && preferred(&accepted, &["gzip"]).is_some() {
                choice = preferred(&accepted, &["gzip", "identity"]);
            }
            assert_eq!(accepts(&list), choice == Some("gzip"), "{list:?}");
        }
    }

    #[test]
    fn repeated_accept_encoding_headers_join() {
        let joined = |values: &[&'static str]| {
            let mut headers = HeaderMap::new();
            for value in values {
                headers.append(header::ACCEPT_ENCODING, HeaderValue::from_static(value));
            }
            wants_gzip(&headers)
        };
        // "gzip;q=0, gzip": of two exact entries the higher quality wins.
        assert!(joined(&["gzip;q=0", "gzip"]));
        assert!(!joined(&["identity", "gzip;q=0"]));
        // "identity, gzip;q=0, gzip": equal quality, identity listed first.
        assert!(!joined(&["identity", "gzip;q=0", "gzip"]));
        assert!(joined(&["br", "gzip"]));
    }

    #[test]
    fn compressible_types() {
        for t in [
            "text/html; charset=utf-8",
            "text/x-component",
            "TEXT/PLAIN",
            "application/json",
            "application/json; charset=utf-8",
            "application/javascript; charset=UTF-8",
            "image/svg+xml",
            "application/manifest+json",
            "application/ld+json",
            "image/x-icon",
            "font/ttf",
            "application/wasm",
            "text/event-stream",
            " text/css",
        ] {
            assert!(compressible(t), "{t}");
        }
        for t in [
            "image/png",
            "image/webp",
            "image/avif",
            "image/jpeg",
            "font/woff2",
            "font/woff",
            "application/octet-stream",
            "application/gzip",
            "application/zip",
            "video/mp4",
            "application/pdf",
            "",
            "text",
            ";text/html",
        ] {
            assert!(!compressible(t), "{t}");
        }
        let mut sorted = COMPRESSIBLE_TYPES.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, COMPRESSIBLE_TYPES, "binary search needs order");
    }

    #[test]
    fn vary_is_added_once() {
        let mut headers = HeaderMap::new();
        add_vary(&mut headers);
        assert_eq!(headers[header::VARY], "Accept-Encoding");
        add_vary(&mut headers);
        assert_eq!(headers.get_all(header::VARY).iter().count(), 1);
        let mut headers = HeaderMap::new();
        headers.insert(
            header::VARY,
            HeaderValue::from_static("rsc, next-router-state-tree"),
        );
        add_vary(&mut headers);
        assert_eq!(
            headers[header::VARY],
            "rsc, next-router-state-tree, Accept-Encoding"
        );
        let mut headers = HeaderMap::new();
        headers.append(header::VARY, HeaderValue::from_static("rsc"));
        headers.append(header::VARY, HeaderValue::from_static("accept-encoding"));
        add_vary(&mut headers);
        assert_eq!(headers.get_all(header::VARY).iter().count(), 2);
        let mut headers = HeaderMap::new();
        headers.insert(header::VARY, HeaderValue::from_static("*"));
        add_vary(&mut headers);
        assert_eq!(headers[header::VARY], "*");
    }

    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// An inner body fed through a channel; sets `dropped` when dropped.
    fn channel_body(
        dropped: Arc<AtomicBool>,
    ) -> (
        tokio::sync::mpsc::UnboundedSender<Result<Bytes, std::io::Error>>,
        Body,
    ) {
        struct Guard(Arc<AtomicBool>);
        impl Drop for Guard {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let stream =
            futures_util::stream::unfold((rx, Guard(dropped)), |(mut rx, guard)| async move {
                rx.recv().await.map(|item| (item, (rx, guard)))
            });
        (tx, Body::from_stream(stream))
    }

    async fn next_frame(body: &mut GzipBody) -> Option<Result<Bytes, axum::Error>> {
        std::future::poll_fn(|cx| Pin::new(&mut *body).poll_frame(cx))
            .await
            .map(|frame| frame.map(|f| f.into_data().expect("data frame")))
    }

    fn pending(body: &mut GzipBody) -> bool {
        let waker = std::task::Waker::noop();
        let mut cx = Context::from_waker(waker);
        Pin::new(body).poll_frame(&mut cx).is_pending()
    }

    /// Each chunk is decodable as soon as it is compressed, before the next
    /// one exists: the client reads the shell while the boundary renders.
    #[tokio::test]
    async fn each_chunk_is_flushed_before_the_next_is_read() {
        let dropped = Arc::new(AtomicBool::new(false));
        let (tx, inner) = channel_body(dropped.clone());
        let mut body = GzipBody::new(inner);
        let mut decoder = flate2::write::GzDecoder::new(Vec::new());
        let chunks = [
            b"<!DOCTYPE html><html><body><div>Loading\xe2\x80\xa6</div>".to_vec(),
            b"<template>Streamed after Suspense boundary</template>".to_vec(),
            vec![b'x'; 10],
        ];
        for chunk in &chunks {
            assert!(pending(&mut body), "polled ahead of the route");
            tx.send(Ok(Bytes::from(chunk.clone()))).unwrap();
            let frame = next_frame(&mut body).await.unwrap().unwrap();
            decoder.write_all(&frame).unwrap();
            decoder.flush().unwrap();
            assert!(
                decoder.get_ref().ends_with(chunk),
                "chunk not decodable yet: {:?}",
                String::from_utf8_lossy(decoder.get_ref())
            );
        }
        drop(tx);
        while let Some(frame) = next_frame(&mut body).await {
            decoder.write_all(&frame.unwrap()).unwrap();
        }
        let decoded = decoder.finish().unwrap();
        assert_eq!(decoded, chunks.concat());
        assert!(body.is_end_stream());
        assert!(dropped.load(Ordering::SeqCst));
    }

    /// A large chunk is fed to zlib in slices: several frames, none larger
    /// than a slice's worth of output plus zlib's buffer.
    #[tokio::test]
    async fn large_chunks_are_sliced() {
        // Poorly compressible: output grows with input.
        let mut state: u32 = 1;
        let big: Vec<u8> = (0..400_000)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                (state & 0xff) as u8
            })
            .collect();
        let mut body = GzipBody::new(Body::from(big.clone()));
        let mut frames = Vec::new();
        while let Some(frame) = next_frame(&mut body).await {
            frames.push(frame.unwrap());
        }
        assert!(frames.len() > 3, "{} frames", frames.len());
        assert!(
            frames.iter().all(|f| f.len() <= 2 * SLICE + 1024),
            "{:?}",
            frames.iter().map(Bytes::len).collect::<Vec<_>>()
        );
        let mut decoded = Vec::new();
        std::io::Read::read_to_end(
            &mut flate2::read::GzDecoder::new(&frames.concat()[..]),
            &mut decoded,
        )
        .unwrap();
        assert_eq!(decoded, big);
    }

    /// A client that goes away drops the response body: the route's body
    /// is dropped with it (its stream is cancelled).
    #[tokio::test]
    async fn dropping_the_body_drops_the_route_body() {
        let dropped = Arc::new(AtomicBool::new(false));
        let (tx, inner) = channel_body(dropped.clone());
        let mut body = GzipBody::new(inner);
        tx.send(Ok(Bytes::from_static(b"first chunk"))).unwrap();
        next_frame(&mut body).await.unwrap().unwrap();
        assert!(!dropped.load(Ordering::SeqCst));
        drop(body);
        assert!(dropped.load(Ordering::SeqCst));
        assert!(tx.send(Ok(Bytes::from_static(b"late"))).is_err());
    }

    /// The route's stream failing ends the compressed stream with the
    /// error, without a gzip trailer.
    #[tokio::test]
    async fn a_failing_route_body_fails_the_stream() {
        let dropped = Arc::new(AtomicBool::new(false));
        let (tx, inner) = channel_body(dropped);
        let mut body = GzipBody::new(inner);
        tx.send(Ok(Bytes::from_static(b"partial"))).unwrap();
        next_frame(&mut body).await.unwrap().unwrap();
        tx.send(Err(std::io::Error::other("render failed")))
            .unwrap();
        assert!(next_frame(&mut body).await.unwrap().is_err());
        assert!(next_frame(&mut body).await.is_none());
    }

    #[tokio::test]
    async fn empty_streams_end_with_a_valid_gzip_member() {
        let (tx, inner) = channel_body(Arc::new(AtomicBool::new(false)));
        drop(tx);
        let mut body = GzipBody::new(inner);
        let mut out = Vec::new();
        while let Some(frame) = next_frame(&mut body).await {
            out.extend_from_slice(&frame.unwrap());
        }
        let mut decoded = Vec::new();
        std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(&out[..]), &mut decoded)
            .unwrap();
        assert!(decoded.is_empty());
    }

    fn response(status: u16, headers: &[(&str, &str)], body: Body) -> Response {
        let mut response = Response::new(body);
        *response.status_mut() = StatusCode::from_u16(status).unwrap();
        for (name, value) in headers {
            response.headers_mut().append(
                header::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        response
    }

    fn gzip_get() -> Negotiated {
        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT_ENCODING, HeaderValue::from_static("gzip"));
        Negotiated::of(&Method::GET, Version::HTTP_11, &headers)
    }

    fn big() -> Body {
        Body::from(vec![b'a'; 4096])
    }

    fn get<'a>(response: &'a Response, name: &str) -> Option<&'a str> {
        response.headers().get(name).map(|v| v.to_str().unwrap())
    }

    #[test]
    fn the_middlewares_order_of_checks() {
        // Not compressible: untouched, no Vary.
        for t in [
            "image/png",
            "image/webp",
            "application/octet-stream",
            "font/woff2",
        ] {
            let r = gzip_get().apply(response(200, &[("content-type", t)], big()));
            assert_eq!(get(&r, "content-encoding"), None, "{t}");
            assert_eq!(get(&r, "vary"), None, "{t}");
        }
        // No Content-Type: untouched.
        let r = gzip_get().apply(response(200, &[], big()));
        assert_eq!(get(&r, "vary"), None);
        // no-transform: untouched, no Vary.
        let r = gzip_get().apply(response(
            200,
            &[
                ("content-type", "text/html"),
                ("cache-control", "private, no-transform"),
            ],
            big(),
        ));
        assert_eq!(get(&r, "content-encoding"), None);
        assert_eq!(get(&r, "vary"), None);
        // Already encoded (a precompressed body): Vary, not compressed again.
        let r = gzip_get().apply(response(
            200,
            &[
                ("content-type", "application/json"),
                ("content-encoding", "br"),
            ],
            big(),
        ));
        assert_eq!(get(&r, "content-encoding"), Some("br"));
        assert_eq!(get(&r, "vary"), Some("Accept-Encoding"));
        // `identity` is no encoding.
        let r = gzip_get().apply(response(
            200,
            &[
                ("content-type", "application/json"),
                ("content-encoding", "identity"),
            ],
            big(),
        ));
        assert_eq!(get(&r, "content-encoding"), Some("gzip"));
        // Below the threshold, by Content-Length or by the body's size.
        let r = gzip_get().apply(response(
            200,
            &[("content-type", "text/plain"), ("content-length", "1023")],
            Body::from(vec![b'a'; 1023]),
        ));
        assert_eq!(get(&r, "content-encoding"), None);
        assert_eq!(get(&r, "vary"), Some("Accept-Encoding"));
        let r = gzip_get().apply(response(
            200,
            &[("content-type", "text/plain")],
            Body::from("tiny"),
        ));
        assert_eq!(get(&r, "content-encoding"), None);
        let r = gzip_get().apply(response(
            200,
            &[("content-type", "text/plain")],
            Body::empty(),
        ));
        assert_eq!(get(&r, "content-encoding"), None);
        // Exactly the threshold is compressed.
        let r = gzip_get().apply(response(
            200,
            &[("content-type", "text/plain")],
            Body::from(vec![b'a'; 1024]),
        ));
        assert_eq!(get(&r, "content-encoding"), Some("gzip"));
        // A stream of unknown size is compressed whatever its size.
        let stream = Body::from_stream(futures_util::stream::iter([Ok::<_, std::io::Error>(
            Bytes::from_static(b"{}"),
        )]));
        let r = gzip_get().apply(response(
            200,
            &[("content-type", "application/json")],
            stream,
        ));
        assert_eq!(get(&r, "content-encoding"), Some("gzip"));
        // Bodyless and partial statuses.
        for status in [204, 304, 206] {
            let r = gzip_get().apply(response(status, &[("content-type", "text/html")], big()));
            assert_eq!(get(&r, "content-encoding"), None, "{status}");
            assert_eq!(get(&r, "vary"), Some("Accept-Encoding"), "{status}");
        }
        let r = gzip_get().apply(response(
            200,
            &[
                ("content-type", "text/html"),
                ("content-range", "bytes 0-9/4096"),
            ],
            big(),
        ));
        assert_eq!(get(&r, "content-encoding"), None);
        // HEAD.
        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT_ENCODING, HeaderValue::from_static("gzip"));
        let head = Negotiated::of(&Method::HEAD, Version::HTTP_11, &headers);
        let r = head.apply(response(
            200,
            &[("content-type", "text/html"), ("content-length", "4096")],
            Body::empty(),
        ));
        assert_eq!(get(&r, "content-encoding"), None);
        assert_eq!(get(&r, "content-length"), Some("4096"));
        // Not accepted.
        let none = Negotiated::of(&Method::GET, Version::HTTP_11, &HeaderMap::new());
        let r = none.apply(response(200, &[("content-type", "text/html")], big()));
        assert_eq!(get(&r, "content-encoding"), None);
        assert_eq!(get(&r, "vary"), Some("Accept-Encoding"));
    }

    #[test]
    fn compressed_responses_change_their_validators() {
        let r = gzip_get().apply(response(
            200,
            &[
                ("content-type", "application/javascript"),
                ("content-length", "4096"),
                ("etag", "\"abc\""),
                ("accept-ranges", "bytes"),
                ("vary", "rsc"),
            ],
            big(),
        ));
        assert_eq!(get(&r, "content-encoding"), Some("gzip"));
        assert_eq!(get(&r, "content-length"), None);
        assert_eq!(get(&r, "accept-ranges"), None);
        assert_eq!(get(&r, "etag"), Some("W/\"abc\""));
        assert_eq!(get(&r, "vary"), Some("rsc, Accept-Encoding"));
        let r = gzip_get().apply(response(
            200,
            &[("content-type", "text/css"), ("etag", "W/\"x\"")],
            big(),
        ));
        assert_eq!(get(&r, "etag"), Some("W/\"x\""));
    }

    /// HTTP/1.0 has no chunked encoding: the compressed response is
    /// delimited by closing the connection.
    #[test]
    fn http_1_0_closes_the_connection() {
        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT_ENCODING, HeaderValue::from_static("gzip"));
        let n = Negotiated::of(&Method::GET, Version::HTTP_10, &headers);
        let r = n.apply(response(
            200,
            &[("content-type", "text/html"), ("content-length", "4096")],
            big(),
        ));
        assert_eq!(get(&r, "content-encoding"), Some("gzip"));
        assert_eq!(get(&r, "connection"), Some("close"));
        assert_eq!(r.version(), Version::HTTP_10);
    }

    #[test]
    fn parse_float_is_javascripts() {
        assert_eq!(parse_float("0.5"), 0.5);
        assert_eq!(parse_float("1"), 1.0);
        assert_eq!(parse_float(" .5x"), 0.5);
        assert_eq!(parse_float("1e-1"), 0.1);
        assert_eq!(parse_float("1e"), 1.0);
        assert!(parse_float("abc").is_nan());
        assert!(parse_float("").is_nan());
        assert_eq!(parse_float("-0"), 0.0);
        assert_eq!(parse_float("Infinity"), f64::INFINITY);
    }
}
