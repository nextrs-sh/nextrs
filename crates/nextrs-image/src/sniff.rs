//! Content sniffing as Next.js does it: `detectContentType`
//! (`next/dist/server/image-optimizer/detect-content-type.js`), whose
//! fallback is the vendored `image-detector` (image-size's validators, in
//! their order), and `is-animated`.

use crate::ImageType;

/// Next only sniffs; it never trusts the upstream's `Content-Type`.
///
/// `Err` where Next's code throws (a PNG signature without an `IHDR`
/// chunk): next start then answers 500.
pub fn detect_content_type(buf: &[u8]) -> Result<Option<ImageType>, SniffError> {
    if buf.is_empty() {
        return Ok(None);
    }
    // `[...].every((b, i) => !b || buffer[i] === b)`: zero bytes are
    // wildcards; a byte past the end compares as `undefined`.
    let magic = |pattern: &[u8], wildcard_zero: bool| {
        pattern
            .iter()
            .enumerate()
            .all(|(i, &b)| (wildcard_zero && b == 0) || buf.get(i) == Some(&b))
    };
    if magic(&[0xff, 0xd8, 0xff], false) {
        return Ok(Some(ImageType::Jpeg));
    }
    if magic(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a], false) {
        return Ok(Some(ImageType::Png));
    }
    if magic(&[0x47, 0x49, 0x46, 0x38], false) {
        return Ok(Some(ImageType::Gif));
    }
    if magic(b"RIFF\0\0\0\0WEBP", true) {
        return Ok(Some(ImageType::Webp));
    }
    if magic(b"<?xml", false) || magic(b"<svg", false) {
        return Ok(Some(ImageType::Svg));
    }
    if magic(b"\0\0\0\0ftypavif", true) {
        return Ok(Some(ImageType::Avif));
    }
    if magic(&[0x00, 0x00, 0x01, 0x00], false) {
        return Ok(Some(ImageType::Ico));
    }
    if magic(b"icns", false) {
        return Ok(Some(ImageType::Icns));
    }
    if magic(&[0x49, 0x49, 0x2a, 0x00], false) {
        return Ok(Some(ImageType::Tiff));
    }
    if magic(&[0x42, 0x4d], false) {
        return Ok(Some(ImageType::Bmp));
    }
    if magic(&[0xff, 0x0a], false) {
        return Ok(Some(ImageType::Jxl));
    }
    if magic(
        &[0, 0, 0, 0, 0x66, 0x74, 0x79, 0x70, 0x68, 0x65, 0x69, 0x63],
        true,
    ) {
        return Ok(Some(ImageType::Heic));
    }
    if magic(b"%PDF-", false) {
        return Ok(Some(ImageType::Pdf));
    }
    if magic(
        &[
            0x00, 0x00, 0x00, 0x0c, 0x6a, 0x50, 0x20, 0x20, 0x0d, 0x0a, 0x87, 0x0a,
        ],
        false,
    ) {
        return Ok(Some(ImageType::Jp2));
    }
    let head = &buf[..buf.len().min(1024)];
    Ok(match detector(head)? {
        Some(Detected::Webp) => Some(ImageType::Webp),
        Some(Detected::Png) => Some(ImageType::Png),
        Some(Detected::Jpg) => Some(ImageType::Jpeg),
        Some(Detected::Gif) => Some(ImageType::Gif),
        Some(Detected::Svg) => Some(ImageType::Svg),
        Some(Detected::Jxl) => Some(ImageType::Jxl),
        Some(Detected::Jp2) => Some(ImageType::Jp2),
        Some(Detected::Tiff) => Some(ImageType::Tiff),
        Some(Detected::Bmp) => Some(ImageType::Bmp),
        Some(Detected::Ico) => Some(ImageType::Ico),
        Some(Detected::Icns) => Some(ImageType::Icns),
        // heif, cur, dds, j2c, ktx, pnm, psd, tga: unsupported.
        Some(Detected::Unsupported) | None => None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SniffError(pub &'static str);

impl std::fmt::Display for SniffError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for SniffError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Detected {
    Webp,
    Png,
    Jpg,
    Gif,
    Svg,
    Jxl,
    Jp2,
    Tiff,
    Bmp,
    Ico,
    Icns,
    Unsupported,
}

/// image-size's validators, in `typeHandlers` order.
#[derive(Clone, Copy)]
enum Handler {
    Bmp,
    Cur,
    Dds,
    Gif,
    Heif,
    Icns,
    Ico,
    J2c,
    Jp2,
    Jpg,
    Jxl,
    JxlStream,
    Ktx,
    Png,
    Pnm,
    Psd,
    Svg,
    Tga,
    Tiff,
    Webp,
}

const ORDER: [Handler; 20] = [
    Handler::Bmp,
    Handler::Cur,
    Handler::Dds,
    Handler::Gif,
    Handler::Heif,
    Handler::Icns,
    Handler::Ico,
    Handler::J2c,
    Handler::Jp2,
    Handler::Jpg,
    Handler::Jxl,
    Handler::JxlStream,
    Handler::Ktx,
    Handler::Png,
    Handler::Pnm,
    Handler::Psd,
    Handler::Svg,
    Handler::Tga,
    Handler::Tiff,
    Handler::Webp,
];

fn detector(t: &[u8]) -> Result<Option<Detected>, SniffError> {
    // Tried first by the first byte, then every handler in order.
    let first = match t[0] {
        56 => Some(Handler::Psd),
        66 => Some(Handler::Bmp),
        68 => Some(Handler::Dds),
        71 => Some(Handler::Gif),
        73 | 77 => Some(Handler::Tiff),
        82 => Some(Handler::Webp),
        105 => Some(Handler::Icns),
        137 => Some(Handler::Png),
        255 => Some(Handler::Jpg),
        _ => None,
    };
    if let Some(handler) = first
        && validate(handler, t)?
    {
        return Ok(Some(detected(handler)));
    }
    for handler in ORDER {
        if validate(handler, t)? {
            return Ok(Some(detected(handler)));
        }
    }
    Ok(None)
}

fn detected(handler: Handler) -> Detected {
    match handler {
        Handler::Bmp => Detected::Bmp,
        Handler::Gif => Detected::Gif,
        Handler::Icns => Detected::Icns,
        Handler::Ico => Detected::Ico,
        Handler::Jp2 => Detected::Jp2,
        Handler::Jpg => Detected::Jpg,
        Handler::Jxl | Handler::JxlStream => Detected::Jxl,
        Handler::Png => Detected::Png,
        Handler::Svg => Detected::Svg,
        Handler::Tiff => Detected::Tiff,
        Handler::Webp => Detected::Webp,
        Handler::Cur
        | Handler::Dds
        | Handler::Heif
        | Handler::J2c
        | Handler::Ktx
        | Handler::Pnm
        | Handler::Psd
        | Handler::Tga => Detected::Unsupported,
    }
}

/// `toUTF8String(t, start, end) === expected` for an ASCII `expected`.
fn ascii_at(t: &[u8], start: usize, expected: &[u8]) -> bool {
    t.get(start..start + expected.len()) == Some(expected)
}

fn u16le(t: &[u8], at: usize) -> Option<u32> {
    Some(u32::from(*t.get(at)?) | (u32::from(*t.get(at + 1)?) << 8))
}

fn u32le(t: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(t.get(at..at + 4)?.try_into().ok()?))
}

fn u32be(t: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(t.get(at..at + 4)?.try_into().ok()?))
}

/// image-size's `findBox`: ISO BMFF boxes from `offset`.
fn find_box(t: &[u8], name: &[u8; 4], mut offset: usize) -> Option<usize> {
    while offset < t.len() {
        if t.len() - offset < 4 {
            return None;
        }
        let size = u32be(t, offset)? as usize;
        if t.len() - offset < size {
            return None;
        }
        if ascii_at(t, offset + 4, name) {
            return Some(offset);
        }
        offset += if size > 0 { size } else { 8 };
    }
    None
}

fn brand_is(t: &[u8], brands: &[&[u8; 4]]) -> bool {
    find_box(t, b"ftyp", 0).is_some_and(|at| brands.iter().any(|b| ascii_at(t, at + 8, *b)))
}

fn validate(handler: Handler, t: &[u8]) -> Result<bool, SniffError> {
    Ok(match handler {
        Handler::Bmp => ascii_at(t, 0, b"BM"),
        Handler::Cur | Handler::Ico => {
            let kind = if matches!(handler, Handler::Cur) {
                2
            } else {
                1
            };
            u16le(t, 0) == Some(0)
                && u16le(t, 4).is_some_and(|n| n != 0)
                && u16le(t, 2) == Some(kind)
        }
        Handler::Dds => u32le(t, 0) == Some(542_327_876),
        Handler::Gif => {
            ascii_at(t, 0, b"GIF8")
                && matches!(t.get(4), Some(b'7' | b'9'))
                && t.get(5) == Some(&b'a')
        }
        Handler::Heif => {
            ascii_at(t, 4, b"ftyp")
                && brand_is(
                    t,
                    &[
                        b"avif", b"mif1", b"msf1", b"heic", b"heix", b"hevc", b"hevx",
                    ],
                )
        }
        Handler::Icns => ascii_at(t, 0, b"icns"),
        Handler::J2c => u32be(t, 0) == Some(4_283_432_785),
        Handler::Jp2 => ascii_at(t, 4, b"jP  ") && brand_is(t, &[b"jp2 "]),
        Handler::Jpg => t.get(..2) == Some(&[0xff, 0xd8]),
        Handler::Jxl => ascii_at(t, 4, b"JXL ") && brand_is(t, &[b"jxl "]),
        Handler::JxlStream => t.get(..2) == Some(&[0xff, 0x0a]),
        Handler::Ktx => ascii_at(t, 1, b"KTX 11") || ascii_at(t, 1, b"KTX 20"),
        Handler::Png => {
            if ascii_at(t, 1, b"PNG\r\n\x1a\n") {
                let chunk = if ascii_at(t, 12, b"CgBI") { 28 } else { 12 };
                if !ascii_at(t, chunk, b"IHDR") {
                    return Err(SniffError("Invalid PNG"));
                }
                true
            } else {
                false
            }
        }
        Handler::Pnm => t.first() == Some(&b'P') && matches!(t.get(1), Some(b'1'..=b'7' | b'F')),
        Handler::Psd => ascii_at(t, 0, b"8BPS"),
        Handler::Svg => {
            let head = String::from_utf8_lossy(&t[..t.len().min(1000)]);
            svg_root().is_match(&head)
        }
        Handler::Tga => u16le(t, 0) == Some(0) && u16le(t, 4) == Some(0),
        Handler::Tiff => {
            t.get(..4) == Some(&[0x49, 0x49, 0x2a, 0x00])
                || t.get(..4) == Some(&[0x4d, 0x4d, 0x00, 0x2a])
        }
        Handler::Webp => {
            ascii_at(t, 0, b"RIFF") && ascii_at(t, 8, b"WEBP") && ascii_at(t, 12, b"VP8")
        }
    })
}

fn svg_root() -> &'static regex::Regex {
    static RE: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(r#"<svg\s([^>"']|"[^"]*"|'[^']*')*>"#).expect("valid pattern")
    })
}

/// `is-animated` (GIF, PNG and WebP only, as Next calls it). `Err` where the
/// package throws: a PNG chunk header past the end of the buffer.
pub fn is_animated(buf: &[u8]) -> Result<bool, SniffError> {
    if buf.get(..3) == Some(b"GIF") {
        return Ok(gif_is_animated(buf));
    }
    if buf.get(..8) == Some(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]) {
        return png_is_animated(buf);
    }
    if buf.get(8..12) == Some(b"WEBP") {
        // The package looks for the bytes "ANIM" anywhere in the file.
        return Ok(buf.windows(4).any(|w| w == b"ANIM"));
    }
    Ok(false)
}

fn gif_is_animated(e: &[u8]) -> bool {
    // JavaScript reads past the end as `undefined`: 0 in bit operations,
    // falsy in conditions.
    let at = |i: usize| e.get(i).copied().unwrap_or(0) as usize;
    let data_blocks_length = |r: usize| {
        let mut t = 0usize;
        while at(r + t) != 0 {
            t += at(r + t) + 1;
        }
        t + 1
    };
    let mut i = 0usize;
    let mut frames = 0;
    let global = at(10) & 128 != 0;
    let size = at(10) & 7;
    i += 6;
    i += 7;
    if global {
        i += 3 * (1usize << (size + 1));
    }
    while frames < 2 && i < e.len() {
        match e[i] {
            44 => {
                frames += 1;
                let local = at(i + 9) & 128 != 0;
                let size = at(i + 9) & 7;
                i += 10;
                if local {
                    i += 3 * (1usize << (size + 1));
                }
                i += data_blocks_length(i + 1) + 1;
            }
            33 => {
                i += 2;
                i += data_blocks_length(i);
            }
            _ => i = e.len(),
        }
    }
    frames > 1
}

fn png_is_animated(e: &[u8]) -> Result<bool, SniffError> {
    let (mut has_actl, mut has_idat, mut has_fdat) = (false, false, false);
    let mut previous: Option<[u8; 4]> = None;
    let mut n = 8usize;
    while n < e.len() {
        // `readUInt32BE` throws when fewer than four bytes remain.
        let len = u32be(e, n).ok_or(SniffError("png chunk length out of range"))? as usize;
        let mut name = [0u8; 4];
        // `slice(n+4, n+8).toString('ascii')`: a short slice is a short name.
        let raw = e.get(n + 4..(n + 8).min(e.len())).unwrap_or(&[]);
        name[..raw.len()].copy_from_slice(raw);
        let name_is = |s: &[u8; 4]| raw.len() == 4 && &name == s;
        let prev_is = |s: &[u8; 4]| previous.as_ref() == Some(s);
        if name_is(b"acTL") {
            has_actl = true;
        } else if name_is(b"IDAT") {
            if !has_actl || (!prev_is(b"fcTL") && !prev_is(b"IDAT")) {
                return Ok(false);
            }
            has_idat = true;
        } else if name_is(b"fdAT") {
            if !has_idat || (!prev_is(b"fcTL") && !prev_is(b"fdAT")) {
                return Ok(false);
            }
            has_fdat = true;
        }
        previous = (raw.len() == 4).then_some(name);
        n += 4 + 4 + len + 4;
    }
    Ok(has_actl && has_idat && has_fdat)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(bytes: &[u8]) -> Option<ImageType> {
        detect_content_type(bytes).unwrap()
    }

    #[test]
    fn magic_numbers() {
        assert_eq!(t(&[0xff, 0xd8, 0xff, 0xe0]), Some(ImageType::Jpeg));
        assert_eq!(t(b"RIFF\x10\0\0\0WEBPVP8 "), Some(ImageType::Webp));
        assert_eq!(t(b"<svg xmlns='x'>"), Some(ImageType::Svg));
        assert_eq!(t(b"\0\0\0\x1cftypavif"), Some(ImageType::Avif));
        assert_eq!(t(b"\0\0\0\x18ftypheic"), Some(ImageType::Heic));
        assert_eq!(t(b"%PDF-1.7"), Some(ImageType::Pdf));
        assert_eq!(t(b""), None);
        assert_eq!(t(b"hello world"), None);
    }

    #[test]
    fn detector_fallback() {
        // SVG after leading whitespace or a doctype (no `<?xml`/`<svg` prefix).
        assert_eq!(t(b"  \n<svg width='1' height='1'>"), Some(ImageType::Svg));
        assert_eq!(
            t(b"<!DOCTYPE html><p><svg viewBox='0 0 1 1'></svg>"),
            Some(ImageType::Svg)
        );
        // Big-endian TIFF is only found by the detector.
        assert_eq!(
            t(&[0x4d, 0x4d, 0x00, 0x2a, 0, 0, 0, 8]),
            Some(ImageType::Tiff)
        );
        // A PNM header comes before the SVG validator: unsupported.
        assert_eq!(t(b"P6 <svg a='b'>"), None);
        // MP4 with an ISO brand: no handler.
        assert_eq!(t(b"\0\0\0\x20ftypisom\0\0\0\0"), None);
        // HEIF brands are unsupported.
        assert_eq!(t(b"\0\0\0\x10ftypmif1\0\0\0\0"), None);
    }

    #[test]
    fn png_signature_without_ihdr_throws() {
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0b];
        png.extend_from_slice(&[0, 0, 0, 0, b'X', b'X', b'X', b'X']);
        // Not the strict PNG magic (last byte differs), but the detector's
        // PNG validator matches bytes 1..8 only when exact; this one isn't.
        assert_eq!(t(&png), None);
        let mut bad = vec![0x00, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        bad.extend_from_slice(&[0, 0, 0, 0, b'X', b'X', b'X', b'X']);
        assert!(detect_content_type(&bad).is_err());
    }

    #[test]
    fn animation() {
        assert!(!is_animated(b"GIF89a").unwrap());
        assert!(is_animated(b"RIFF\0\0\0\0WEBPVP8XxxxxANIM").unwrap());
        assert!(!is_animated(b"RIFF\0\0\0\0WEBPVP8 ").unwrap());
        // Truncated PNG chunk header: the package throws.
        let png = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0, 0];
        assert!(is_animated(&png).is_err());
    }
}
