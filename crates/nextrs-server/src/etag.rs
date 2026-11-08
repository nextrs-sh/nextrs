//! Port of `generateETag` from `next/dist/server/lib/etag.js`.
//!
//! Next.js hashes the JavaScript string, so the input is UTF-16 code units
//! and the length suffix is the UTF-16 length, not the byte length.

pub(crate) fn generate_etag(payload: &str) -> String {
    let units: Vec<u16> = payload.encode_utf16().collect();
    format!(
        "\"{}{}\"",
        to_base36(fnv1a52(&units)),
        to_base36(units.len() as u64)
    )
}

fn fnv1a52(units: &[u16]) -> u64 {
    let (mut v0, mut v1, mut v2, mut v3): (u64, u64, u64, u64) = (0x2325, 0x8422, 0x9ce4, 0xcbf2);
    for &unit in units {
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
    let joined = |name: header::HeaderName| -> Option<String> {
        let values: Vec<&str> = request
            .get_all(name)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .collect();
        // Node joins repeated headers with ", "; an empty value is falsy.
        Some(values.join(", ")).filter(|v| !v.is_empty())
    };
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
}
