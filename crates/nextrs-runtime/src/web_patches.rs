//! Exact-text edits of deno_web's JavaScript, applied to its sources
//! before the runtime loads them. They remove per-call work that Phase 3A
//! profiles of server rendering found hot and that `next start` does not
//! do. deno_web is pinned (`=0.291.0`); if an edit no longer matches its
//! source (an upgrade), that file is loaded unchanged and a warning is
//! logged. The unit tests below fail first.
//!
//! ## Streams: Node's cached release errors (`06_streams.js`)
//!
//! Releasing a stream reader or writer rejects its pending promises with a
//! `TypeError`. deno_web constructs a new error, with a captured stack
//! trace, on every release: two per default-reader release (the `closed`
//! promise and pending reads) and one per writer release. Next.js releases
//! readers on every rendered response (async iteration and piping between
//! the React stream and its transforms); in Phase 3A profiles of Pages
//! Router SSR those constructions were 6.3 % of all CPU samples
//! (`Builtin_ErrorConstructor` → stack capture → source positions), and
//! `next start` spends nothing there: Node 24 creates each of these errors
//! once, lazily, and reuses the instance for every release
//! (`lazyReadableReleasedError`, `lazyReadableReleasingError`,
//! `lazyWritableReleasedError` in `internal/webstreams`). The edit does the
//! same. Messages stay deno_web's; only the identity of the error objects
//! changes, which then matches Node. `tests/streams_release.rs` checks the
//! observable behavior against Node's output.
//!
//! ## Not patched: `TextEncoder.encode`
//!
//! A JavaScript fast path for short ASCII strings (React encodes ~140
//! markup fragments per Pages Router request, 98.6 % ASCII and at most 64
//! units) cut native allocations by ~8 % but showed no CPU or latency gain
//! over this streams edit in the Phase 3A A/B
//! (`benchmarks/results/phase3a/ab-webpatch`): the result array still gets
//! an off-heap backing store, because rusty_v8 builds V8 with
//! `v8_typed_array_max_size_in_heap = 0`. It was dropped;
//! `tests/text_encoder.rs` still holds encodings to Node's.

use std::sync::{Arc, OnceLock};

use deno_core::{Extension, ExtensionFileSource};

struct FilePatch {
    specifier: &'static str,
    /// Text that occurs once; `prelude` is inserted before it.
    anchor: &'static str,
    prelude: &'static str,
    /// (original, replacement); each original must occur exactly once.
    edits: &'static [(&'static str, &'static str)],
}

const STREAMS: FilePatch = FilePatch {
    specifier: "ext:deno_web/06_streams.js",
    anchor: "/**\n * @param {ReadableStreamBYOBReader} reader\n */\nfunction readableStreamBYOBReaderRelease(reader) {",
    prelude: "// NextRS: one instance per kind, as in Node (see web_patches.rs).
let nextrsReaderReleasedError;
function nextrsReaderReleased() {
  return nextrsReaderReleasedError ??= new TypeError(
    \"Reader was released and can no longer be used to monitor the stream's closedness.\",
  );
}
let nextrsReleasingReaderError;
function nextrsReleasingReader() {
  return nextrsReleasingReaderError ??= new TypeError(\"The reader was released.\");
}
let nextrsWriterReleasedError;
function nextrsWriterReleased() {
  return nextrsWriterReleasedError ??= new TypeError(
    \"The writer has already been released.\",
  );
}

",
    edits: &[
        (
            "function readableStreamBYOBReaderRelease(reader) {\n  readableStreamReaderGenericRelease(reader);\n  const e = new TypeError(\"The reader was released.\");",
            "function readableStreamBYOBReaderRelease(reader) {\n  readableStreamReaderGenericRelease(reader);\n  const e = nextrsReleasingReader();",
        ),
        (
            "function readableStreamDefaultReaderRelease(reader) {\n  readableStreamReaderGenericRelease(reader);\n  const e = new TypeError(\"The reader was released.\");",
            "function readableStreamDefaultReaderRelease(reader) {\n  readableStreamReaderGenericRelease(reader);\n  const e = nextrsReleasingReader();",
        ),
        (
            "  reader[_closedPromise].reject(\n    new TypeError(\n      \"Reader was released and can no longer be used to monitor the stream's closedness.\",\n    ),\n  );",
            "  reader[_closedPromise].reject(nextrsReaderReleased());",
        ),
        (
            "  const releasedError = new TypeError(\n    \"The writer has already been released.\",\n  );",
            "  const releasedError = nextrsWriterReleased();",
        ),
    ],
};

const PATCHES: [&FilePatch; 1] = [&STREAMS];

/// The patched source, or `None` if the pinned text was not found.
fn patch(file: &FilePatch, source: &str) -> Option<String> {
    if source.matches(file.anchor).count() != 1 {
        return None;
    }
    let mut out = source.replacen(file.anchor, &format!("{}{}", file.prelude, file.anchor), 1);
    for (from, to) in file.edits {
        if out.matches(from).count() != 1 {
            return None;
        }
        out = out.replacen(from, to, 1);
    }
    Some(out)
}

/// Replaces the patched files in `web` (the extension returned by
/// `deno_web::init`). Each edit is computed once per process and shared by
/// every isolate.
pub(crate) fn apply(web: &mut Extension) {
    static PATCHED: OnceLock<Vec<Option<Arc<str>>>> = OnceLock::new();
    let files = web.lazy_loaded_js_files.to_mut();
    let patched = PATCHED.get_or_init(|| {
        PATCHES
            .iter()
            .map(|p| {
                let source = files
                    .iter()
                    .find(|f| f.specifier == p.specifier)?
                    .load()
                    .ok()?;
                let patched = patch(p, source.as_str());
                if patched.is_none() {
                    tracing::warn!(
                        file = p.specifier,
                        "deno_web source changed; NextRS edit not applied"
                    );
                }
                patched.map(Arc::from)
            })
            .collect()
    });
    for (p, code) in PATCHES.iter().zip(patched) {
        if let (Some(code), Some(file)) =
            (code, files.iter_mut().find(|f| f.specifier == p.specifier))
        {
            *file = ExtensionFileSource::new_computed(p.specifier, code.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn original(specifier: &str) -> String {
        let mut web = deno_web::deno_web::init(
            Arc::new(deno_web::BlobStore::default()),
            None,
            false,
            deno_web::InMemoryBroadcastChannel::default(),
        );
        let file = web
            .lazy_loaded_js_files
            .to_mut()
            .iter()
            .find(|f| f.specifier == specifier)
            .expect("deno_web ships the file as a lazily loaded script")
            .load()
            .unwrap();
        file.as_str().to_owned()
    }

    #[test]
    fn the_pinned_streams_source_is_patched() {
        let patched = patch(&STREAMS, &original(STREAMS.specifier)).expect("every edit matches");
        assert_eq!(
            patched
                .matches("new TypeError(\"The reader was released.\")")
                .count(),
            1
        );
        assert_eq!(
            patched
                .matches("const e = nextrsReleasingReader();")
                .count(),
            2
        );
        assert_eq!(
            patched.matches("reject(nextrsReaderReleased());").count(),
            1
        );
        assert_eq!(
            patched
                .matches("const releasedError = nextrsWriterReleased();")
                .count(),
            1
        );
        // The constructions that remain are the cached ones.
        assert_eq!(
            patched
                .matches("\"Reader was released and can no longer be used to monitor the stream's closedness.\"")
                .count(),
            1
        );
        assert!(patched.is_ascii(), "extension sources must be ASCII");
    }

    #[test]
    fn a_changed_source_is_left_alone() {
        assert!(patch(&STREAMS, "function unrelated() {}").is_none());
        let source = original(STREAMS.specifier)
            .replace("const e = new TypeError(\"The reader was released.\");", "");
        assert!(patch(&STREAMS, &source).is_none());
    }
}
