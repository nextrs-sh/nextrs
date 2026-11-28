//! Port of `generateETag` from `next/dist/server/lib/etag.js`.
//!
//! Next.js hashes the JavaScript string, so the input is UTF-16 code units
//! and the length suffix is the UTF-16 length, not the byte length.

pub(crate) fn generate_etag(payload: &str) -> String {
    // Over the code units as they are produced: collecting them first cost
    // a temporary of twice the payload's size per prerendered page (RE-5B).
    let mut length = 0u64;
    let hash = fnv1a52(payload.encode_utf16().inspect(|_| length += 1));
    format!("\"{}{}\"", to_base36(hash), to_base36(length))
}

fn fnv1a52(units: impl Iterator<Item = u16>) -> u64 {
    let (mut v0, mut v1, mut v2, mut v3): (u64, u64, u64, u64) = (0x2325, 0x8422, 0x9ce4, 0xcbf2);
    for unit in units {
        v0 ^= u64::from(unit);
        let t0 = v0 * 435;
        let mut t1 = v1 * 435;
        let mut t2 = v2 * 435;
        let mut t3 = v3 * 435;
        t2 += v0 << 8;
        t3 += v1 << 8;
        t1 += t0 >> 16;
        v0 = t0 & 0xffff;
        t2 += t1 >> 16;
        v1 = t1 & 0xffff;
        v3 = (t3 + (t2 >> 16)) & 0xffff;
        v2 = t2 & 0xffff;
    }
    (v3 & 15) * 281_474_976_710_656 + v2 * 4_294_967_296 + v1 * 65_536 + (v0 ^ (v3 >> 4))
}

fn to_base36(mut value: u64) -> String {
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if value == 0 {
        return "0".into();
    }
    let mut out = Vec::new();
    while value > 0 {
        out.push(DIGITS[(value % 36) as usize]);
        value /= 36;
    }
    out.reverse();
    String::from_utf8(out).expect("base36 digits are ASCII")
}

/// The `fresh` package next start judges every conditional request with
/// (`sendRenderResult` for pages, `send` for files): not fresh without a
/// validator or when the request says `Cache-Control: no-cache` (as
/// browsers and Node's fetch do for a forced revalidation); `If-None-Match`
/// matches strongly or weakly in either direction, and `*` alone always;
/// `If-Modified-Since` needs a Last-Modified no later than it. The status
/// does not matter (a 404 page with a matching ETag is a 304 too).
pub(crate) fn fresh(
    request: &axum::http::HeaderMap,
    etag: Option<&str>,
    last_modified: Option<&str>,
) -> bool {
    use axum::http::header;
    let joined = |name: header::HeaderName| joined(request, &name);
    let since = joined(header::IF_MODIFIED_SINCE);
    let none_match = joined(header::IF_NONE_MATCH);
    if since.is_none() && none_match.is_none() {
        return false;
    }
    if joined(header::CACHE_CONTROL).is_some_and(|cc| has_no_cache(&cc)) {
        return false;
    }
    if let Some(none_match) = none_match.as_deref()
        && none_match != "*"
    {
        let Some(etag) = etag else {
            return false;
        };
        let weak = format!("W/{etag}");
        let matched = none_match
            .split(',')
            .map(|token| token.trim_matches(' '))
            .any(|token| token == etag || token == weak || format!("W/{token}") == etag);
        if !matched {
            return false;
        }
    }
    if let Some(since) = since {
        let date = |text: &str| httpdate::parse_http_date(text).ok();
        let not_modified = match (last_modified.and_then(date), date(&since)) {
            (Some(modified), Some(since)) => modified <= since,
            _ => false,
        };
        if !not_modified {
            return false;
        }
    }
    true
}

/// A request header's values joined with ", " as Node joins repeated
/// headers; `None` when absent or empty (falsy in JavaScript).
fn joined(request: &axum::http::HeaderMap, name: &axum::http::HeaderName) -> Option<String> {
    let values: Vec<&str> = request
        .get_all(name)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .collect();
    Some(values.join(", ")).filter(|v| !v.is_empty())
}

/// RFC 9110 section 8.8.3.2, the strong comparison: both entity tags
/// strong (no `W/`) and identical.
pub(crate) fn strong_eq(a: &str, b: &str) -> bool {
    !a.starts_with("W/") && !b.starts_with("W/") && a == b
}

/// Whether a file request's preconditions fail (RFC 9110 sections 13.1.1,
/// 13.1.4 and 13.2.2 steps 1 and 2), so the answer is 412.
///
/// - `If-Match` takes precedence: `*` holds for any existing file;
///   otherwise one listed entity tag must equal the file's with the strong
///   comparison. Static files carry a weak ETag (`W/"<size>-<mtime>"`, as
///   with next start), so a listed tag never matches them and only `*`
///   passes.
/// - Without `If-Match`, `If-Unmodified-Since` holds when the file was
///   modified no later than the date (compared in whole seconds, the
///   resolution of `Last-Modified`). A value that is not an HTTP-date is
///   ignored; a file without a modification time fails it.
///
/// next start (`send`) compares `If-Match` weakly, accepts any date
/// `Date.parse` reads, and turns the resulting 412 into a 500 error page;
/// NextRS answers 412 (engineering/compatibility.md, intended differences).
pub(crate) fn precondition_failed(
    request: &axum::http::HeaderMap,
    etag: Option<&str>,
    modified: Option<std::time::SystemTime>,
) -> bool {
    use axum::http::header;
    if let Some(if_match) = joined(request, &header::IF_MATCH) {
        let tokens: Vec<&str> = if_match
            .split(',')
            .map(|t| t.trim_matches(|c| c == ' ' || c == '\t'))
            .filter(|t| !t.is_empty())
            .collect();
        if !tokens.is_empty() {
            let matched = tokens
                .iter()
                .any(|token| *token == "*" || etag.is_some_and(|etag| strong_eq(token, etag)));
            return !matched;
        }
    }
    let Some(since) = joined(request, &header::IF_UNMODIFIED_SINCE) else {
        return false;
    };
    let Ok(since) = httpdate::parse_http_date(since.trim()) else {
        return false;
    };
    let seconds = |t: std::time::SystemTime| {
        t.duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    };
    match modified {
        Some(modified) => seconds(modified) > seconds(since),
        None => true,
    }
}

/// `/(?:^|,)\s*?no-cache\s*?(?:,|$)/` from `fresh`.
fn has_no_cache(cache_control: &str) -> bool {
    cache_control
        .split(',')
        .any(|directive| directive.trim_matches(|c: char| c.is_whitespace()) == "no-cache")
}

#[cfg(test)]
mod tests {
    use super::*;

    // Expected values computed with Next.js 16.4.3:
    //   node -e 'console.log(require("next/dist/server/lib/etag").generateETag(s))'
    fn request(pairs: &[(&str, &str)]) -> axum::http::HeaderMap {
        let mut map = axum::http::HeaderMap::new();
        for (name, value) in pairs {
            map.append(
                axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
                value.parse().unwrap(),
            );
        }
        map
    }

    /// Cases checked against next start 16.4.3 (and the fresh package).
    #[test]
    fn freshness_follows_the_fresh_package() {
        let etag = Some("\"abc\"");
        assert!(!super::fresh(&request(&[]), etag, None));
        assert!(super::fresh(
            &request(&[("if-none-match", "\"abc\"")]),
            etag,
            None
        ));
        assert!(super::fresh(
            &request(&[("if-none-match", "W/\"abc\"")]),
            etag,
            None
        ));
        assert!(super::fresh(
            &request(&[("if-none-match", "\"x\", \"abc\"")]),
            etag,
            None
        ));
        assert!(!super::fresh(
            &request(&[("if-none-match", "\"x\"")]),
            etag,
            None
        ));
        // A forced revalidation is never answered from the validator.
        assert!(!super::fresh(
            &request(&[("if-none-match", "\"abc\""), ("cache-control", "no-cache")]),
            etag,
            None
        ));
        assert!(!super::fresh(
            &request(&[
                ("if-none-match", "\"abc\""),
                ("cache-control", "max-age=0, no-cache")
            ]),
            etag,
            None
        ));
        assert!(super::fresh(
            &request(&[
                ("if-none-match", "\"abc\""),
                ("cache-control", "no-cache-x")
            ]),
            etag,
            None
        ));
        // `*` alone matches anything; inside a list it is a token like any other.
        assert!(super::fresh(
            &request(&[("if-none-match", "*")]),
            etag,
            None
        ));
        assert!(super::fresh(
            &request(&[("if-none-match", "*")]),
            None,
            None
        ));
        assert!(!super::fresh(
            &request(&[("if-none-match", "\"a\", *")]),
            etag,
            None
        ));
        // A weak validator stored matches its strong spelling in the request.
        assert!(super::fresh(
            &request(&[("if-none-match", "\"abc\"")]),
            Some("W/\"abc\""),
            None
        ));
        let modified = Some("Sun, 08 Nov 2026 10:00:00 GMT");
        assert!(super::fresh(
            &request(&[("if-modified-since", "Sun, 08 Nov 2026 10:00:00 GMT")]),
            None,
            modified
        ));
        assert!(!super::fresh(
            &request(&[("if-modified-since", "Sun, 08 Nov 2026 09:59:59 GMT")]),
            None,
            modified
        ));
        assert!(!super::fresh(
            &request(&[("if-modified-since", "Sun, 08 Nov 2026 10:00:00 GMT")]),
            None,
            None
        ));
    }

    #[test]
    fn matches_next_js_for_ascii() {
        assert_eq!(generate_etag(""), "\"78p3gz236i0\"");
        assert_eq!(generate_etag("hello world"), "\"stglysbf6mb\"");
    }

    #[test]
    fn hashes_utf16_code_units() {
        // "…" is one UTF-16 unit but three UTF-8 bytes; "😀" is two units.
        assert_eq!(generate_etag("Loading…"), "\"yy15vje2q08\"");
        assert_eq!(generate_etag("😀"), "\"c26v2dkjuu2\"");
    }

    #[test]
    fn if_match_uses_the_strong_comparison() {
        let weak = Some("W/\"13e69-1a201c1de77\"");
        let strong = Some("\"abc\"");
        let failed =
            |pairs: &[(&str, &str)], etag| precondition_failed(&request(pairs), etag, None);
        // Strong tags: equal passes, different fails, lists pass on any.
        assert!(!failed(&[("if-match", "\"abc\"")], strong));
        assert!(failed(&[("if-match", "\"abd\"")], strong));
        assert!(!failed(&[("if-match", "\"x\", \"abc\"")], strong));
        assert!(!failed(
            &[("if-match", "\"x\""), ("if-match", "\"abc\"")],
            strong
        ));
        // Weak tags never match, in either position, even when identical.
        assert!(failed(&[("if-match", "W/\"abc\"")], strong));
        assert!(failed(&[("if-match", "W/\"13e69-1a201c1de77\"")], weak));
        assert!(failed(&[("if-match", "\"13e69-1a201c1de77\"")], weak));
        // `*` holds for an existing representation, with or without a tag.
        assert!(!failed(&[("if-match", "*")], weak));
        assert!(!failed(&[("if-match", "*")], None));
        // No validator: only `*` can hold.
        assert!(failed(&[("if-match", "\"abc\"")], None));
        // An empty value is no precondition.
        assert!(!failed(&[("if-match", " ")], strong));
        assert!(!failed(&[], strong));
    }

    #[test]
    fn if_unmodified_since_compares_whole_seconds() {
        let modified = httpdate::parse_http_date("Sun, 08 Nov 2026 10:00:00 GMT").unwrap()
            + std::time::Duration::from_millis(700);
        let failed = |since: &str, modified| {
            precondition_failed(&request(&[("if-unmodified-since", since)]), None, modified)
        };
        assert!(!failed("Sun, 08 Nov 2026 10:00:00 GMT", Some(modified)));
        assert!(!failed("Mon, 09 Nov 2026 10:00:00 GMT", Some(modified)));
        assert!(failed("Sun, 08 Nov 2026 09:59:59 GMT", Some(modified)));
        // The obsolete HTTP-date formats are dates too (RFC 9110 5.6.7).
        assert!(failed("Sunday, 08-Nov-26 09:59:59 GMT", Some(modified)));
        assert!(failed("Sun Nov  8 09:59:59 2026", Some(modified)));
        // Not an HTTP-date: ignored (next start's Date.parse reads ISO dates).
        assert!(!failed("2000-01-01", Some(modified)));
        assert!(!failed("garbage", Some(modified)));
        // No modification time: the condition cannot hold.
        assert!(failed("Sun, 08 Nov 2026 10:00:00 GMT", None));
    }

    #[test]
    fn if_match_takes_precedence_over_if_unmodified_since() {
        let modified = Some(httpdate::parse_http_date("Sun, 08 Nov 2026 10:00:00 GMT").unwrap());
        let past = ("if-unmodified-since", "Sat, 01 Jan 2000 00:00:00 GMT");
        let future = ("if-unmodified-since", "Fri, 01 Jan 2100 00:00:00 GMT");
        let strong = Some("\"abc\"");
        // A passing If-Match ignores a failing date, a failing one a passing date.
        assert!(!precondition_failed(
            &request(&[("if-match", "\"abc\""), past]),
            strong,
            modified
        ));
        assert!(precondition_failed(
            &request(&[("if-match", "\"x\""), future]),
            strong,
            modified
        ));
        assert!(!precondition_failed(
            &request(&[("if-match", "*"), past]),
            strong,
            modified
        ));
    }
}
