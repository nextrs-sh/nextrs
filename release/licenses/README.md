# Third-party license texts for release archives

`scripts/package-release.sh` writes two files into every release archive,
with `scripts/third-party-licenses.mjs`:

- `THIRD-PARTY.md`: every component the binary contains, its version, its
  license, and where its license text comes from;
- `THIRD-PARTY-LICENSES.txt`: the license and notice texts themselves.

The Rust crates' texts come from their published sources (the Cargo
registry, as locked by `Cargo.lock`). This directory holds the texts that
cannot be found there:

| Path | What |
|---|---|
| `native/` | V8 and the C/C++ libraries that the `v8` crate (rusty_v8 150.4.0) compiles into its static library: V8, ICU, abseil-cpp, libc++, libc++abi, libunwind, llvm-libc, fast_float, simdutf, highway, dragonbox, FP16, PartitionAlloc (under Chromium's license), and rusty_v8 itself. The published `v8` crate ships none of these files. |
| `spdx/` | SPDX texts of MIT, BSD-3-Clause and Apache-2.0 (license-list-data v3.27.0), for the 18 crates that publish no license file (among them `v8`, `deno_core`, `deno_web` and `deno_webidl`). Their authors are named as copyright holders. |
| `SOURCES.tsv` | Where each file in `native/` and `spdx/` came from: repository, revision, path. |
| `fetch.sh` | Fetches them again (`release/licenses/fetch.sh <dir>`). |

The revisions in `SOURCES.tsv` are the submodule commits of rusty_v8's
`v150.4.0` tag (`5c15a699`). PartitionAlloc has no license file of its
own: its sources point to Chromium's `LICENSE`, fetched at a fixed
Chromium commit.

**When `v8` is upgraded,** update the revisions in `fetch.sh` from the new
rusty_v8 tag's submodules (`gh api repos/denoland/rusty_v8/contents/<path>?ref=<tag>`),
run it and commit the result. A crate added without a license file and
without a declared license that has an SPDX text here makes
`third-party-licenses.mjs`, and so packaging, fail.

## The Deno JavaScript compiled into the binary

`crates/nextrs-runtime/js/vendor/` holds 27 JavaScript files, unmodified,
from two crates (see its `README.md` and `src/embedded_js.rs`):

| Crate | Version | crates.io SHA-256 (as in `Cargo.lock`) | Files |
|---|---|---|---|
| `deno_web` | 0.291.0 | `8658d7955894408e10b53018f651435ca6418b873be2e0b7d98224f3493c20f2` | the 26 `*.js` files at the crate root |
| `deno_webidl` | 0.260.0 | `7de03e5cc0901f948592021dc9046b2c5ebe80956014eacd9198d2b05d55ec89` | `00_webidl.js` |

Both are MIT-licensed, `Copyright 2018-2026 the Deno authors`; each file
carries that header, and the archive's `THIRD-PARTY-LICENSES.txt` has the
license text under both crates' entries.

**Checking provenance** without the local registry:

```bash
curl -sSfLO https://static.crates.io/crates/deno_web/deno_web-0.291.0.crate
sha256sum deno_web-0.291.0.crate      # must equal the Cargo.lock checksum above
tar -xzf deno_web-0.291.0.crate
for f in crates/nextrs-runtime/js/vendor/deno_web/*.js; do cmp "$f" "deno_web-0.291.0/$(basename "$f")"; done
```

(and the same for `deno_webidl`). The unit tests in `embedded_js.rs` do the
byte comparison against the registry copy on every `cargo test`.

**Upgrading `deno_web` or `deno_webidl`:**

1. Change the pinned version (`=x.y.z`) in the workspace manifest.
2. Copy every `*.js` file that the new crate declares (`esm`, `js`,
   `lazy_loaded_esm`, `lazy_loaded_js` in its `extension!`) into
   `js/vendor/<crate>/`, unmodified, and remove the ones it no longer has.
3. Update the file lists in `src/embedded_js.rs` and the versions in
   `js/vendor/README.md` and in the table above.
4. Run `cargo test -p nextrs-runtime embedded_js` (byte equality, and no
   extension file loaded from a path) and `cargo test -p nextrs-runtime
   web_patches` (NextRS's documented edit of `06_streams.js` still applies).
5. Build the release binary and run `scripts/runtime-file-audit.sh`: it
   must report no build-machine path.
