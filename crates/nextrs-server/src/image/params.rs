//! `ImageOptimizerCache.validateParams` and `getCacheKey` (Next.js
//! 16.4.3, `next/dist/server/image-optimizer.js`), production mode: the
//! same checks in the same order, with the same messages.

use base64::Engine;
use sha2::{Digest, Sha256};

use super::patterns::Patterns;
use crate::routing::query::{JsMap, Value};

/// Next's `CACHE_VERSION`.
const CACHE_VERSION: u32 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Params {
    /// The `url` parameter as given for a local image; normalized
    /// (`new URL(url).toString()`) for a remote one.
    pub href: String,
    pub is_absolute: bool,
    pub is_static: bool,
    pub width: u32,
    pub quality: u8,
    /// Negotiated output type, `""` to keep the source's type.
    pub mime_type: String,
}

pub(crate) enum Invalid {
    /// 400 with this body.
    Message(String),
    /// Next's own code throws (an `Accept` parameter without a value, an
    /// undecodable `url` path): next start answers 500.
    Throws,
}

fn message(s: impl Into<String>) -> Invalid {
    Invalid::Message(s.into())
}

/// A query value as `parsedUrl.query` holds it.
enum Param<'a> {
    Missing,
    One(&'a str),
    Many,
}

fn param<'a>(query: &'a JsMap, key: &str) -> Param<'a> {
    match query.get(key) {
        None => Param::Missing,
        Some(Value::Str(s)) if s.is_empty() => Param::Missing,
        Some(Value::Str(s)) => Param::One(s),
        Some(Value::Arr(_)) => Param::Many,
    }
}

/// `parseInt(digits, 10)` for an all-digit string, as a JavaScript number
/// would print it in the messages (integers below 2^53).
fn parse_digits(s: &str) -> u64 {
    s.bytes().fold(0u64, |acc, b| {
        acc.saturating_mul(10).saturating_add(u64::from(b - b'0'))
    })
}

pub(crate) fn validate(
    query: &JsMap,
    accept: Option<&str>,
    config: &super::Config,
) -> Result<Params, Invalid> {
    let url = match param(query, "url") {
        Param::Missing => return Err(message("\"url\" parameter is required")),
        Param::Many => return Err(message("\"url\" parameter cannot be an array")),
        Param::One(url) => url,
    };
    if url.encode_utf16().count() > 3072 {
        return Err(message("\"url\" parameter is too long"));
    }
    if url.starts_with("//") {
        return Err(message(
            "\"url\" parameter cannot be a protocol-relative URL (//)",
        ));
    }
    let (href, is_absolute) = if url.starts_with('/') {
        let base = url::Url::parse("http://n").expect("valid base");
        // `decodeURIComponent(parseUrl(url)?.pathname ?? '')`: a pathname
        // that does not decode throws.
        let pathname = base
            .join(url)
            .map(|u| u.path().to_owned())
            .unwrap_or_default();
        let decoded = decode_uri_component(&pathname).ok_or(Invalid::Throws)?;
        if is_recursive(&decoded) {
            return Err(message("\"url\" parameter cannot be recursive"));
        }
        if !config.patterns.local_allows(url) {
            return Err(message("\"url\" parameter is not allowed"));
        }
        (url.to_owned(), false)
    } else {
        let Ok(parsed) = url::Url::parse(url) else {
            return Err(message("\"url\" parameter is invalid"));
        };
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(message("\"url\" parameter is invalid"));
        }
        if !config.patterns.remote_allows(&parsed) {
            return Err(message("\"url\" parameter is not allowed"));
        }
        (parsed.to_string(), true)
    };

    let w = match param(query, "w") {
        Param::Missing => return Err(message("\"w\" parameter (width) is required")),
        Param::Many => return Err(message("\"w\" parameter (width) cannot be an array")),
        Param::One(w) => w,
    };
    if !w.bytes().all(|b| b.is_ascii_digit()) {
        return Err(message(
            "\"w\" parameter (width) must be an integer greater than 0",
        ));
    }
    let q = match param(query, "q") {
        Param::Missing => return Err(message("\"q\" parameter (quality) is required")),
        Param::Many => return Err(message("\"q\" parameter (quality) cannot be an array")),
        Param::One(q) => q,
    };
    if !q.bytes().all(|b| b.is_ascii_digit()) {
        return Err(message(
            "\"q\" parameter (quality) must be an integer between 1 and 100",
        ));
    }
    let width = parse_digits(w);
    if width == 0 {
        return Err(message(
            "\"w\" parameter (width) must be an integer greater than 0",
        ));
    }
    if !config.sizes.iter().any(|&s| u64::from(s) == width) {
        return Err(message(format!(
            "\"w\" parameter (width) of {width} is not allowed"
        )));
    }
    let quality = parse_digits(q);
    if !(1..=100).contains(&quality) {
        return Err(message(
            "\"q\" parameter (quality) must be an integer between 1 and 100",
        ));
    }
    if let Some(qualities) = &config.qualities
        && !qualities
            .iter()
            .any(|&allowed| u64::from(allowed) == quality)
    {
        return Err(message(format!(
            "\"q\" parameter (quality) of {q} is not allowed"
        )));
    }
    let mime_type = super::accept::supported_mime_type(&config.formats, accept)
        .map_err(|()| Invalid::Throws)?;
    let base = &config.base_path;
    let is_static = url.starts_with(&format!("{base}/_next/static/media"))
        || url.starts_with(&format!("{base}/_next/static/immutable/media"));
    Ok(Params {
        href,
        is_absolute,
        is_static,
        width: width as u32,
        quality: quality as u8,
        mime_type,
    })
}

/// `/\/_next\/image($|\/)/`
fn is_recursive(pathname: &str) -> bool {
    pathname
        .match_indices("/_next/image")
        .any(|(i, m)| matches!(pathname.as_bytes().get(i + m.len()), None | Some(b'/')))
}

/// `decodeURIComponent`: `None` where it throws (a malformed escape or
/// bytes that are not UTF-8).
pub(crate) fn decode_uri_component(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// `getHash([CACHE_VERSION, href, width, quality, mimeType])`: SHA-256 over
/// the parts as strings, base64url without padding.
pub(crate) fn cache_key(params: &Params) -> String {
    let mut hash = Sha256::new();
    hash.update(CACHE_VERSION.to_string());
    hash.update(&params.href);
    hash.update(params.width.to_string());
    hash.update(params.quality.to_string());
    hash.update(&params.mime_type);
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash.finalize())
}

impl Patterns {
    fn local_allows(&self, url: &str) -> bool {
        self.local_match(url)
    }

    fn remote_allows(&self, url: &url::Url) -> bool {
        #[cfg(feature = "negative-controls")]
        if super::controls::skip_remote_auth() {
            return true;
        }
        self.remote_match(url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recursion() {
        assert!(is_recursive("/_next/image"));
        assert!(is_recursive("/a/_next/image/b"));
        assert!(!is_recursive("/_next/imagex"));
        assert!(!is_recursive("/_next/images/a.png"));
    }

    #[test]
    fn decoding() {
        assert_eq!(decode_uri_component("/a%20b").as_deref(), Some("/a b"));
        assert_eq!(decode_uri_component("/%E6%97%A5").as_deref(), Some("/日"));
        assert_eq!(decode_uri_component("/%"), None);
        assert_eq!(decode_uri_component("/%zz"), None);
        assert_eq!(decode_uri_component("/%ff"), None);
    }

    #[test]
    fn cache_key_matches_next() {
        // node -e 'console.log(require("next/dist/server/image-optimizer")
        //   .ImageOptimizerCache.getCacheKey({href:"/photos/a.jpg",width:640,
        //   quality:75,mimeType:"image/webp"}))'
        let params = Params {
            href: "/photos/a.jpg".into(),
            is_absolute: false,
            is_static: false,
            width: 640,
            quality: 75,
            mime_type: "image/webp".into(),
        };
        assert_eq!(
            cache_key(&params),
            "6F_LYYFTeCBudFecgqwbkZMm2XmiVyy_5BwGqeCkIoo"
        );
    }
}
