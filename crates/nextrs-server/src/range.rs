//! Byte-range requests for static files (`/_next/static` and `public/`),
//! RFC 9110 section 14.
//!
//! next start serves these files with the `send` package, which answers
//! one byte range with 206 and ignores several (the full 200). NextRS does
//! the same for valid single ranges and differs where `send` behind Next
//! produces invalid responses or the RFC says otherwise:
//!
//! - an unsatisfiable range is a 416 with `Content-Range: bytes */<size>`
//!   (next start: a 500 error page carrying that Content-Range);
//! - a malformed or invalid ranges-specifier (`bytes=abc`, `bytes=5-2`) is
//!   ignored: the full 200 (next start: 500, as for unsatisfiable);
//! - Range is evaluated for GET only; HEAD ignores it and describes the
//!   full response (RFC 9110 section 14.2; next start answers HEAD with
//!   206 headers);
//! - `If-Range` uses the strong comparison: a weak entity tag, which every
//!   static file has (`W/"<size>-<mtime>"`), never matches, and a date
//!   matches only a Last-Modified that is exactly equal and at least one
//!   second old (`send` matches weak tags by substring and any date no
//!   earlier than Last-Modified);
//! - the partial bytes are always those of the identity representation, and
//!   a 206 is never compressed. When the full response would be gzip, an
//!   `If-Range` is evaluated as false: a client resuming a download it
//!   received compressed gets the whole (compressed) response again instead
//!   of identity bytes spliced onto a gzip prefix. next start compresses
//!   the partial body, so its Content-Range no longer describes the bytes.
//!
//! Several ranges are not served as `multipart/byteranges`: the full
//! response is sent instead, which is always a valid answer to a Range
//! request (and bounds the work one request can ask for). Only the first
//! two range-specs are ever looked at.

use std::time::SystemTime;

use axum::http::HeaderMap;
use axum::http::header;

/// What a request's Range header selects from a representation of `len`
/// bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Selection {
    /// No (usable) Range: the full representation, 200.
    Full,
    /// Bytes `start..=end`, 206.
    Partial { start: u64, end: u64 },
    /// A valid range none of whose bytes exist: 416.
    Unsatisfiable,
}

/// One `range-spec`.
enum Spec {
    Int { first: u64, last: Option<u64> },
    Suffix(u64),
}

/// `1*DIGIT`, saturating (a position past any file is as good as the
/// largest one).
fn digits(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some(text.bytes().fold(0u64, |n, b| {
        n.saturating_mul(10).saturating_add(u64::from(b - b'0'))
    }))
}

fn spec(text: &str) -> Option<Spec> {
    let (first, last) = text.split_once('-')?;
    if first.is_empty() {
        return Some(Spec::Suffix(digits(last)?));
    }
    let first = digits(first)?;
    let last = if last.is_empty() {
        None
    } else {
        Some(digits(last)?)
    };
    // An int-range whose last-pos is below its first-pos is invalid.
    if last.is_some_and(|last| last < first) {
        return None;
    }
    Some(Spec::Int { first, last })
}

/// The selection of a `Range` header value for `len` bytes.
pub(crate) fn select(value: &str, len: u64) -> Selection {
    // ranges-specifier = range-unit "=" range-set; the unit is
    // case-insensitive, and an unknown unit means the header is ignored.
    let Some((unit, set)) = value.trim().split_once('=') else {
        return Selection::Full;
    };
    if !unit.trim_end().eq_ignore_ascii_case("bytes") {
        return Selection::Full;
    }
    // 1#range-spec: comma-separated, optional whitespace, empty elements
    // allowed.
    let mut specs = set
        .split(',')
        .map(|s| s.trim_matches([' ', '\t']))
        .filter(|s| !s.is_empty());
    let (Some(only), None) = (specs.next(), specs.next()) else {
        // None (invalid) or several (not served): the full response.
        return Selection::Full;
    };
    match spec(only) {
        None => Selection::Full,
        Some(Spec::Int { first, .. }) if first >= len => Selection::Unsatisfiable,
        Some(Spec::Int { first, last }) => Selection::Partial {
            start: first,
            end: last.map_or(len - 1, |last| last.min(len - 1)),
        },
        Some(Spec::Suffix(0)) => Selection::Unsatisfiable,
        // A suffix of an empty representation selects nothing that a
        // Content-Range can name: the (empty) full response.
        Some(Spec::Suffix(_)) if len == 0 => Selection::Full,
        Some(Spec::Suffix(n)) => Selection::Partial {
            start: len - n.min(len),
            end: len - 1,
        },
    }
}

/// `If-Range` (RFC 9110 section 13.1.5): true when the request may get a
/// partial response for the representation described by `etag` and
/// `modified`. Absent: true.
pub(crate) fn if_range_holds(
    request: &HeaderMap,
    etag: Option<&str>,
    modified: Option<SystemTime>,
) -> bool {
    let mut values = request.get_all(header::IF_RANGE).iter();
    let value = match (values.next(), values.next()) {
        (None, _) => return true,
        (Some(one), None) => match one.to_str() {
            Ok(value) => value.trim(),
            Err(_) => return false,
        },
        // A list is not a valid If-Range.
        _ => return false,
    };
    if value.starts_with('"') || value.starts_with("W/") {
        // Strong comparison: both strong and identical.
        return !value.starts_with("W/")
            && etag.is_some_and(|etag| !etag.starts_with("W/") && etag == value);
    }
    let (Some(modified), Ok(date)) = (modified, httpdate::parse_http_date(value)) else {
        return false;
    };
    // Last-Modified is sent with one-second resolution; it is a strong
    // validator only when at least a second older than the response.
    let modified_s = modified
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let date_s = date
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(u64::MAX, |d| d.as_secs());
    let strong = SystemTime::now()
        .duration_since(modified)
        .is_ok_and(|age| age.as_secs() >= 1);
    strong && modified_s == date_s
}

/// `bytes <start>-<end>/<len>`.
pub(crate) fn content_range(start: u64, end: u64, len: u64) -> String {
    format!("bytes {start}-{end}/{len}")
}

/// `bytes */<len>`.
pub(crate) fn unsatisfied_range(len: u64) -> String {
    format!("bytes */{len}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use Selection::*;

    #[test]
    fn single_ranges() {
        let len = 1000;
        for (value, expected) in [
            ("bytes=0-0", Partial { start: 0, end: 0 }),
            ("bytes=0-99", Partial { start: 0, end: 99 }),
            (
                "bytes=999-999",
                Partial {
                    start: 999,
                    end: 999,
                },
            ),
            (
                "bytes=100-199",
                Partial {
                    start: 100,
                    end: 199,
                },
            ),
            ("bytes=0-999", Partial { start: 0, end: 999 }),
            ("bytes=0-", Partial { start: 0, end: 999 }),
            (
                "bytes=990-",
                Partial {
                    start: 990,
                    end: 999,
                },
            ),
            (
                "bytes=-10",
                Partial {
                    start: 990,
                    end: 999,
                },
            ),
            (
                "bytes=-1",
                Partial {
                    start: 999,
                    end: 999,
                },
            ),
            ("bytes=-1000", Partial { start: 0, end: 999 }),
            ("bytes=-5000", Partial { start: 0, end: 999 }),
            (
                "bytes=500-5000",
                Partial {
                    start: 500,
                    end: 999,
                },
            ),
            (
                "bytes=0-99999999999999999999999",
                Partial { start: 0, end: 999 },
            ),
            ("BYTES=0-9", Partial { start: 0, end: 9 }),
            (" bytes=0-9 ", Partial { start: 0, end: 9 }),
            ("bytes= 0-9", Partial { start: 0, end: 9 }),
            ("bytes=0-9,", Partial { start: 0, end: 9 }),
            ("bytes=,0-9", Partial { start: 0, end: 9 }),
        ] {
            assert_eq!(select(value, len), expected, "{value}");
        }
    }

    #[test]
    fn unsatisfiable_ranges() {
        for value in [
            "bytes=1000-",
            "bytes=1000-1000",
            "bytes=5000-6000",
            "bytes=-0",
            "bytes=99999999999999999999999-",
        ] {
            assert_eq!(select(value, 1000), Unsatisfiable, "{value}");
        }
        // Nothing of an empty file can be selected by position.
        assert_eq!(select("bytes=0-0", 0), Unsatisfiable);
        assert_eq!(select("bytes=0-", 0), Unsatisfiable);
        assert_eq!(select("bytes=-0", 0), Unsatisfiable);
        assert_eq!(select("bytes=-5", 0), Full);
    }

    #[test]
    fn ignored_headers_select_everything() {
        for value in [
            "",
            "bytes",
            "bytes=",
            "bytes=,",
            "bytes=abc",
            "bytes=5-2",
            "bytes=-",
            "bytes=1-2-3",
            "bytes=+1-2",
            "bytes=0x10-20",
            "bytes=1 -2",
            "bytes=-1-",
            "items=0-5",
            "bytes 0-5",
            "bytes=0-0,5-9",
            "bytes=0-0,abc",
            "bytes=0-0,0-0,0-0",
        ] {
            assert_eq!(select(value, 1000), Full, "{value}");
        }
    }

    fn if_range(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(header::IF_RANGE, value.parse().unwrap());
        headers
    }

    #[test]
    fn if_range_is_strong() {
        let modified = SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(1_795_000_000_500);
        let date = httpdate::fmt_http_date(modified);
        assert!(if_range_holds(&HeaderMap::new(), None, None));
        // Weak entity tags never match, even identical ones.
        assert!(!if_range_holds(
            &if_range("W/\"abc\""),
            Some("W/\"abc\""),
            Some(modified)
        ));
        assert!(!if_range_holds(
            &if_range("\"abc\""),
            Some("W/\"abc\""),
            Some(modified)
        ));
        assert!(if_range_holds(
            &if_range("\"abc\""),
            Some("\"abc\""),
            Some(modified)
        ));
        assert!(!if_range_holds(
            &if_range("\"abd\""),
            Some("\"abc\""),
            Some(modified)
        ));
        // Dates: exactly Last-Modified (to the second).
        assert!(if_range_holds(&if_range(&date), None, Some(modified)));
        let later = httpdate::fmt_http_date(modified + std::time::Duration::from_secs(1));
        assert!(!if_range_holds(&if_range(&later), None, Some(modified)));
        let earlier = httpdate::fmt_http_date(modified - std::time::Duration::from_secs(1));
        assert!(!if_range_holds(&if_range(&earlier), None, Some(modified)));
        assert!(!if_range_holds(&if_range(&date), None, None));
        assert!(!if_range_holds(
            &if_range("yesterday"),
            None,
            Some(modified)
        ));
        // A file modified just now has no strong Last-Modified yet.
        let now = SystemTime::now();
        assert!(!if_range_holds(
            &if_range(&httpdate::fmt_http_date(now)),
            None,
            Some(now)
        ));
        // Repeated: not a valid If-Range.
        let mut headers = if_range(&date);
        headers.append(header::IF_RANGE, date.parse().unwrap());
        assert!(!if_range_holds(&headers, None, Some(modified)));
    }

    #[test]
    fn content_range_values() {
        assert_eq!(content_range(0, 9, 100), "bytes 0-9/100");
        assert_eq!(unsatisfied_range(100), "bytes */100");
    }
}
