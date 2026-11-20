# Publishing a NextRS release (maintainers)

**Nothing here runs automatically.** Every step that publishes needs the
maintainer's explicit decision:

- creating a tag;
- creating a GitHub release;
- uploading assets.

`.github/workflows/release.yml` builds, tests and packages, but publishes
nothing.

## Rule: publish the binary that was tested

Builds are not bit-for-bit reproducible: a rebuild of the same commit
differs by a few kilobytes of code. What is published is therefore the
**binary** that went through the release gates, identified by its
SHA-256. For v0.1.0-rc.1 that binary was built by the release workflow
(`workflow_dispatch` run 44646067537 on `main` at `711d77f`), not on a
developer machine:

```text
binary sha256  44f04e586c7dc9eb0b8f69dfd992143f725472aa5f3619df5a4039d77d2c6956
```

The archive around it may be repackaged at a later commit (release notes,
licenses) with `--built-from 711d77f`; `package-release.sh` accepts that
only if `Cargo.toml`, `Cargo.lock` and `crates/` are unchanged since
`711d77f`. Every repackaged archive must pass the clean-install test
again before it is published, because the archive bytes changed.

`release/WITHDRAWN.txt` lists builds that must never be published.
`package-release.sh` refuses them, and refuses release notes that name
them.

## Gates that are per binary, and per archive

| Gate | Re-run when |
|---|---|
| Runtime file audit (`scripts/runtime-file-audit.sh`) | the binary changes |
| Clean installs (`scripts/clean-install-containers.sh`) | the archive changes |
| Compatibility matrix, Velasia certification, release regression | the binary changes |
| Six-hour soak | the binary changes |
| CI (`ci.yml`, `release.yml`) | the release commit changes |

**Test hermeticity.** A smoke test on the build machine cannot find a
dependency on the build machine's files: they are all there. Two checks
can:

- `scripts/runtime-file-audit.sh` records every path the binary touches
  (strace) and fails on any under the Cargo home, rustup, a Cargo target
  directory or the source tree, even though those reads would succeed;
- `scripts/clean-install-containers.sh` runs the archive in fresh
  Ubuntu 22.04, Ubuntu 24.04 and Debian 12 containers whose only mounts are
  the archive, prebuilt applications and curl-only test scripts (from
  `scripts/clean-install-bundle.sh`).

Both run in `release.yml`; both can be run locally (the containers with
`--engine podman` or `docker`).

## Steps for v0.1.0-rc.1

1. **Close every blocking gate.** The final validation report's gate
   matrix must show every gate as passed on binary `44f04e58…6956`.
2. **Check the source tree.** The tree at the release commit must be
   clean, and CI (`ci.yml` and `release.yml`) must be green on that commit.
3. **Package the tested binary at the release commit:**

   ```bash
   gh run download 44646067537 -R nextrs-sh/nextrs -n nextrs-v0.1.0-rc.1-linux-x86_64 -D ci
   (cd ci && sha256sum -c *.sha256 && tar -xzf *.tar.gz)
   sha256sum ci/nextrs-v0.1.0-rc.1-linux-x86_64/nextrs   # 44f04e58...6956
   scripts/package-release.sh --binary ci/nextrs-v0.1.0-rc.1-linux-x86_64/nextrs \
     --version 0.1.0-rc.1 --out dist --toolchain 1.98.0 --built-from 711d77f
   ```

   Then check, in `dist/*.manifest.json`: `binary_sha256` is
   `44f04e58…6956`, `binary_commit` is `711d77f…`, `dirty: false`.
   Packaging is deterministic: the archive checksum must equal the one
   committed in `release/checksums/`.
4. **Clean-install the archive:**

   ```bash
   scripts/clean-install-bundle.sh --archive dist/nextrs-v0.1.0-rc.1-linux-x86_64.tar.gz --out /tmp/ci-bundle
   scripts/clean-install-containers.sh --bundle /tmp/ci-bundle --results /tmp/ci-results
   ```

5. **Tag and push,** with authorization:

   ```bash
   git tag -a v0.1.0-rc.1 -m "NextRS v0.1.0-rc.1"
   git push origin v0.1.0-rc.1
   ```

   The tag push runs the release workflow on the tagged commit. Its own
   binary is a rebuild and is **not** the one to publish; the run is an
   independent check of the pipeline, the audit and the clean installs.
6. **Create the GitHub release as a prerelease,** with authorization:

   ```bash
   gh release create v0.1.0-rc.1 --prerelease --title "NextRS v0.1.0-rc.1" \
     --notes-file release/v0.1.0-rc.1-release-notes.md \
     dist/nextrs-v0.1.0-rc.1-linux-x86_64.tar.gz \
     dist/nextrs-v0.1.0-rc.1-linux-x86_64.tar.gz.sha256 \
     dist/nextrs-v0.1.0-rc.1-linux-x86_64.manifest.json
   ```

   It must stay marked prerelease: a release candidate is never the
   latest stable release.

**Never publish to crates.io.** The workspace has `publish = false`, and
the `nextrs` name there belongs to an unrelated project. There is no
container image.
