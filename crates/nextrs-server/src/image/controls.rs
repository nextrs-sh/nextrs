//! Deliberate defects for the image suite's negative controls
//! (`NEXTRS_IMAGE_NEGATIVE_CONTROL`). Compiled only with the
//! `negative-controls` feature, which release builds never enable: the
//! suites run a separately built binary to prove they catch each defect.

fn active(name: &str) -> bool {
    static CONTROL: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    CONTROL
        .get_or_init(|| std::env::var("NEXTRS_IMAGE_NEGATIVE_CONTROL").unwrap_or_default())
        .split(',')
        .any(|c| c == name)
}

/// `swap-cache`: every entry lands under one key, so one image (width,
/// format) is served for another.
pub(crate) fn cache_key(key: String) -> String {
    if active("swap-cache") {
        "0".repeat(64)
    } else {
        key
    }
}

/// `wrong-dimensions`: one pixel wider than asked.
pub(crate) fn width(width: u32) -> u32 {
    if active("wrong-dimensions") {
        width + 1
    } else {
        width
    }
}

/// `skip-remote-auth`: `remotePatterns` are not checked.
pub(crate) fn skip_remote_auth() -> bool {
    active("skip-remote-auth")
}

/// `wrong-content-type`: every image is labelled PNG.
pub(crate) fn content_type(actual: &str) -> &str {
    if active("wrong-content-type") {
        "image/png"
    } else {
        actual
    }
}

/// `corrupt-bytes`: the encoded output is damaged before it is cached.
pub(crate) fn corrupt(bytes: &mut [u8]) {
    if active("corrupt-bytes") {
        let start = bytes.len() / 3;
        for b in bytes.iter_mut().skip(start).take(64) {
            *b ^= 0x5a;
        }
    }
}
