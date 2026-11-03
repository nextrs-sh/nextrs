//! RSC cache-busting check, ported from Next.js 16.4.3
//! `shared/lib/router/utils/cache-busting-search-param.js` and
//! `client/components/router-reducer/set-cache-busting-search-param.js`.
//!
//! An RSC request must carry `_rsc=<hash of the router headers>`; otherwise
//! Next.js answers 307 to the URL with the expected value. This keeps CDNs
//! that ignore `Vary` from serving a payload for the wrong router state.

use axum::http::HeaderMap;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

pub(crate) const RSC_QUERY: &str = "_rsc";
pub(crate) const STATE_TREE_HEADER: &str = "next-router-state-tree";
pub(crate) const NEXT_URL_HEADER: &str = "next-url";

pub(crate) struct RouterHeaders<'a> {
    pub prefetch: Option<&'a str>,
    pub segment_prefetch: Option<&'a str>,
    pub state_tree: Option<&'a str>,
    pub next_url: Option<&'a str>,
}

impl<'a> RouterHeaders<'a> {
    pub fn from_headers(headers: &'a HeaderMap, prefetch: &str, segment: &str) -> Self {
        let get = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
        Self {
            // base-server only forwards the prefetch kinds it understands.
            prefetch: get(prefetch).filter(|v| matches!(*v, "1" | "2" | "3")),
            segment_prefetch: get(segment),
            state_tree: get(STATE_TREE_HEADER),
            next_url: get(NEXT_URL_HEADER),
        }
    }

    fn input(&self) -> Option<String> {
        if self.prefetch.is_none_or(|p| p == "0")
            && self.segment_prefetch.is_none()
            && self.state_tree.is_none()
            && self.next_url.is_none()
        {
            return None;
        }
        Some(
            [
                self.prefetch.unwrap_or("0"),
                self.segment_prefetch.unwrap_or("0"),
                self.state_tree.unwrap_or("0"),
                self.next_url.unwrap_or("0"),
            ]
            .join(","),
        )
    }

    /// SHA-256 truncated to 96 bits, base64url without padding.
    pub fn expected_hash(&self) -> String {
        self.input().map_or_else(String::new, |input| {
            URL_SAFE_NO_PAD.encode(&Sha256::digest(input.as_bytes())[..12])
        })
    }

    /// 5-character djb2 form still accepted for clients without WebCrypto.
    fn legacy_hash(&self) -> String {
        self.input().map_or_else(String::new, |input| {
            let mut hash: u32 = 5381;
            for unit in input.encode_utf16() {
                hash = (hash << 5).wrapping_add(hash).wrapping_add(u32::from(unit));
            }
            let mut digits = to_base36(hash);
            digits.truncate(5);
            digits
        })
    }

    pub fn matches(&self, actual: Option<&str>) -> bool {
        match actual {
            None => false,
            Some(actual) => actual == self.expected_hash() || actual == self.legacy_hash(),
        }
    }
}

/// First `_rsc` value in a raw query string (decoded like `URLSearchParams`).
pub(crate) fn query_hash(query: Option<&str>) -> Option<String> {
    query?.split('&').find_map(|pair| {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        (key == RSC_QUERY).then(|| {
            percent_encoding::percent_decode_str(&value.replace('+', " "))
                .decode_utf8_lossy()
                .into_owned()
        })
    })
}

/// Rebuilds the query with `_rsc=<hash>` the way Next.js does, preserving the
/// other pairs verbatim.
pub(crate) fn redirect_target(path: &str, query: Option<&str>, hash: &str) -> String {
    let mut pairs: Vec<&str> = query
        .unwrap_or("")
        .split('&')
        .filter(|pair| !pair.is_empty() && !pair.starts_with("_rsc="))
        .collect();
    let own = if hash.is_empty() {
        RSC_QUERY.to_owned()
    } else {
        format!("{RSC_QUERY}={hash}")
    };
    pairs.push(&own);
    format!("{path}?{}", pairs.join("&"))
}

fn to_base36(mut value: u32) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn headers<'a>(
        prefetch: Option<&'a str>,
        segment: Option<&'a str>,
        tree: Option<&'a str>,
        url: Option<&'a str>,
    ) -> RouterHeaders<'a> {
        RouterHeaders {
            prefetch,
            segment_prefetch: segment,
            state_tree: tree,
            next_url: url,
        }
    }

    // Vectors from Next.js 16.4.3 computeCacheBustingSearchParam /
    // computeLegacyCacheBustingSearchParam.
    #[test]
    fn hashes_match_next_js() {
        let cases = [
            (headers(None, None, None, None), "", ""),
            (
                headers(Some("1"), Some("/_tree"), None, None),
                "_i_aeImnuN6u1u1r",
                "1r34m",
            ),
            (
                headers(Some("1"), None, None, None),
                "HEAvJdHAQJrUAe2K",
                "bl95g",
            ),
            (
                headers(None, None, Some("%5B%22%22%5D"), None),
                "WCzv4JOXuLb8w6j7",
                "1ch7l",
            ),
            (
                headers(Some("2"), Some("/__PAGE__"), Some("tree"), Some("/books")),
                "ZCV9ZZp7CUUxve5u",
                "l4ef9",
            ),
        ];
        for (input, expected, legacy) in cases {
            assert_eq!(input.expected_hash(), expected);
            assert_eq!(input.legacy_hash(), legacy);
            assert!(input.matches(Some(expected)));
        }
    }

    #[test]
    fn missing_param_never_matches() {
        assert!(!headers(None, None, None, None).matches(None));
        assert!(headers(None, None, None, None).matches(Some("")));
    }

    #[test]
    fn redirect_preserves_other_pairs() {
        assert_eq!(redirect_target("/", None, ""), "/?_rsc");
        assert_eq!(
            redirect_target("/", Some("a=1&_rsc=old&q=a%20b"), "h"),
            "/?a=1&q=a%20b&_rsc=h"
        );
        assert_eq!(query_hash(Some("a=1&_rsc")), Some(String::new()));
        assert_eq!(query_hash(Some("a=1")), None);
    }
}
