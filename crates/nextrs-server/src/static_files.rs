//! Maps URL paths onto files below a fixed root without escaping it.

use std::path::{Path, PathBuf};

use percent_encoding::percent_decode_str;

/// Resolves the percent-encoded `url_path` (relative to the mount point)
/// to a regular file inside `root`, which must already be canonical.
///
/// Rejected: invalid UTF-8 after decoding, NUL, backslashes, `.`/`..`
/// segments (encoded or not), and anything whose canonical path (after
/// following symlinks) is outside `root`.
pub(crate) async fn resolve(root: &Path, url_path: &str) -> Option<PathBuf> {
    let decoded = percent_decode_str(url_path).decode_utf8().ok()?;
    if decoded.contains('\0') || decoded.contains('\\') {
        return None;
    }
    let mut path = root.to_path_buf();
    for segment in decoded.split('/') {
        match segment {
            "" => {}
            "." | ".." => return None,
            segment => path.push(segment),
        }
    }
    let canonical = tokio::fs::canonicalize(&path).await.ok()?;
    if !canonical.starts_with(root) {
        return None;
    }
    let metadata = tokio::fs::metadata(&canonical).await.ok()?;
    metadata.is_file().then_some(canonical)
}

/// Content types as sent by `next start` (the `send` package) for the
/// extensions Next.js emits; unknown extensions are served as bytes.
pub(crate) fn content_type(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase);
    match extension.as_deref() {
        Some("js" | "mjs") => "application/javascript; charset=UTF-8",
        Some("css") => "text/css; charset=UTF-8",
        Some("json" | "map") => "application/json; charset=UTF-8",
        Some("html") => "text/html; charset=UTF-8",
        Some("txt") => "text/plain; charset=UTF-8",
        Some("xml") => "application/xml",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("wasm") => "application/wasm",
        Some("rsc") => "text/x-component",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("nextrs-static-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("static/chunks")).unwrap();
        std::fs::write(root.join("static/chunks/app.js"), "ok").unwrap();
        std::fs::write(root.join("secret.json"), "{}").unwrap();
        root.canonicalize().unwrap()
    }

    #[tokio::test]
    async fn serves_files_inside_root() {
        let root = temp_root("inside");
        let static_root = root.join("static");
        assert_eq!(
            resolve(&static_root, "chunks/app.js").await,
            Some(static_root.join("chunks/app.js"))
        );
        assert!(resolve(&static_root, "chunks//app.js").await.is_some());
        assert_eq!(
            resolve(&static_root, "chunks").await,
            None,
            "directories are not files"
        );
        assert_eq!(resolve(&static_root, "chunks/missing.js").await, None);
    }

    #[tokio::test]
    async fn rejects_traversal() {
        let root = temp_root("traversal");
        let static_root = root.join("static");
        for attempt in [
            "../secret.json",
            "chunks/../../secret.json",
            "%2e%2e/secret.json",
            "%2E%2E%2Fsecret.json",
            "chunks/%2e%2e/%2e%2e/secret.json",
            "..%5csecret.json",
            "chunks/app.js%00.png",
            "./chunks/app.js",
            "%ff",
        ] {
            assert_eq!(resolve(&static_root, attempt).await, None, "{attempt}");
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn rejects_symlinks_leaving_root() {
        let root = temp_root("symlink");
        let static_root = root.join("static");
        std::os::unix::fs::symlink(root.join("secret.json"), static_root.join("leak.json"))
            .unwrap();
        assert_eq!(resolve(&static_root, "leak.json").await, None);
    }

    #[test]
    fn content_types_match_next_start() {
        assert_eq!(
            content_type(Path::new("a/b.js")),
            "application/javascript; charset=UTF-8"
        );
        assert_eq!(content_type(Path::new("x.CSS")), "text/css; charset=UTF-8");
        assert_eq!(content_type(Path::new("font.woff2")), "font/woff2");
        assert_eq!(content_type(Path::new("noext")), "application/octet-stream");
    }
}
