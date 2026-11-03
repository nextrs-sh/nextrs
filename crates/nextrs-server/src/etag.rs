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

#[cfg(test)]
mod tests {
    use super::*;

    // Expected values computed with Next.js 16.4.3:
    //   node -e 'console.log(require("next/dist/server/lib/etag").generateETag(s))'
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
