//! Compiles deno_web's and deno_webidl's JavaScript into the binary.
//!
//! deno_core 0.413 records an extension's lazily loaded files by their
//! absolute path on the build machine (`LoadedFromFsDuringSnapshot`). It
//! expects them to be read while a startup snapshot is built. NextRS
//! creates its runtimes without a snapshot, so each isolate start would
//! read `~/.cargo/registry/src/.../deno_web-*/` from disk. A release binary
//! built that way fails on every other machine, with "Failed to initialize
//! a JsRuntime: No such file or directory".
//!
//! [`embed`] swaps each such entry for the vendored copy in
//! `js/vendor/`, which is compiled in. It runs before `web_patches::apply`,
//! so the patches edit the embedded text. The tests below check that the
//! copies match the pinned crates byte for byte, and that no file in either
//! extension is still loaded from a path.

use deno_core::{Extension, ExtensionFileSource, FastStaticString};

macro_rules! vendored {
    ($ext:literal: $($file:literal),* $(,)?) => {
        &[$((
            concat!("ext:", $ext, "/", $file),
            deno_core::ascii_str_include!(concat!("../js/vendor/", $ext, "/", $file)),
        )),*]
    };
}

type Table = &'static [(&'static str, FastStaticString)];

/// deno_web 0.291.0: its `lazy_loaded_js` and `lazy_loaded_esm` files.
pub(crate) const DENO_WEB: Table = vendored!("deno_web":
    "00_infra.js",
    "00_url.js",
    "01_broadcast_channel.js",
    "01_console.js",
    "01_dom_exception.js",
    "01_mimesniff.js",
    "01_urlpattern.js",
    "02_event.js",
    "02_structured_clone.js",
    "02_timers.js",
    "03_abort_signal.js",
    "04_global_interfaces.js",
    "05_base64.js",
    "06_streams.js",
    "08_text_encoding.js",
    "09_file.js",
    "10_filereader.js",
    "12_location.js",
    "13_message_port.js",
    "14_compression.js",
    "15_performance.js",
    "16_image_data.js",
    "17_geometry.js",
    "18_css_stylesheet.js",
    "locks.js",
    "webtransport.js",
);

/// deno_webidl 0.260.0: its one `lazy_loaded_js` file.
pub(crate) const DENO_WEBIDL: Table = vendored!("deno_webidl": "00_webidl.js");

/// Replaces every file of `ext` that would be read from a build-machine
/// path with its compiled-in copy from `table`. A file missing from the
/// table is left alone; the tests catch it.
pub(crate) fn embed(ext: &mut Extension, table: Table) {
    for files in [
        &mut ext.js_files,
        &mut ext.esm_files,
        &mut ext.lazy_loaded_js_files,
        &mut ext.lazy_loaded_esm_files,
    ] {
        if files.iter().all(ExtensionFileSource::is_runtime_loadable) {
            continue;
        }
        for file in files.to_mut().iter_mut() {
            if file.is_runtime_loadable() {
                continue;
            }
            if let Some((specifier, code)) = table.iter().find(|(s, _)| *s == file.specifier) {
                *file = ExtensionFileSource::new(specifier, *code);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;

    fn web() -> Extension {
        deno_web::deno_web::init(
            Arc::new(deno_web::BlobStore::default()),
            None,
            false,
            deno_web::InMemoryBroadcastChannel::default(),
        )
    }

    fn all_files(ext: &Extension) -> Vec<ExtensionFileSource> {
        [
            &ext.js_files,
            &ext.esm_files,
            &ext.lazy_loaded_js_files,
            &ext.lazy_loaded_esm_files,
        ]
        .into_iter()
        .flat_map(|files| files.iter().cloned())
        .collect()
    }

    /// Reads the pinned crate's file from the path deno_core recorded. This
    /// works only where the crate sources exist, which is the case for
    /// `cargo test`.
    fn assert_matches_upstream(ext: Extension, table: Table) {
        let upstream: Vec<_> = all_files(&ext)
            .into_iter()
            .filter(|f| !f.is_runtime_loadable())
            .collect();
        assert_eq!(
            upstream.len(),
            table.len(),
            "the vendored table and the crate disagree on the file list"
        );
        for file in upstream {
            let (_, code) = table
                .iter()
                .find(|(s, _)| *s == file.specifier)
                .unwrap_or_else(|| panic!("{} is not vendored", file.specifier));
            let original = file.load().expect("the pinned crate's source");
            assert!(
                original.as_str() == code.as_ref(),
                "js/vendor copy of {} differs from the pinned crate",
                file.specifier
            );
        }
    }

    #[test]
    fn deno_web_copies_match_the_pinned_crate() {
        assert_matches_upstream(web(), DENO_WEB);
    }

    #[test]
    fn deno_webidl_copy_matches_the_pinned_crate() {
        assert_matches_upstream(deno_webidl::deno_webidl::init(), DENO_WEBIDL);
    }

    #[test]
    fn no_file_is_loaded_from_a_build_machine_path() {
        let mut web = web();
        embed(&mut web, DENO_WEB);
        let mut webidl = deno_webidl::deno_webidl::init();
        embed(&mut webidl, DENO_WEBIDL);
        for ext in [&web, &webidl] {
            for file in all_files(ext) {
                assert!(
                    file.is_runtime_loadable(),
                    "{} would be read from the build machine at run time",
                    file.specifier
                );
            }
        }
    }
}
