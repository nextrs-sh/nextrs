# Publishing a NextRS release (maintainers)

**Nothing here runs automatically.** Every step that publishes needs the
maintainer's explicit decision:

- creating a tag;
- creating a GitHub release;
- uploading assets.

`.github/workflows/release.yml` builds, tests and packages, but publishes
nothing.

## Rule: publish the archive that was tested

Builds are not bit-for-bit reproducible. A rebuild of the same commit
differs by a few kilobytes of code. The archive to publish is therefore
the one whose binary went through the release gates. Its binary checksum
is in the gate record:

- the six-hour soak;
- the Velasia certification;
- the release regression;
- the clean-install smoke test.

Do not substitute a rebuild, including the release workflow's own
artifact. Use that artifact to verify the pipeline and clean installs.

## The tested archive's records

These files record the archive that passed the gates:

- `release/checksums/`: its SHA-256, and the binary's;
- `release/manifests/`: its manifest.

**Which commit they refer to.** The archive was packaged at commit
`b06745d`, the manifest's `commit`. The commit that adds these records
changes nothing else.

**Repackaging is safe.** The archive is deterministic, so repackaging the
same binary at `b06745d` gives the same SHA-256. Repackaging at a later
commit changes `BUILD-INFO.json` and the archive's checksum, but not the
binary's. Either way:

- `binary_sha256` must stay `621da204…`;
- whatever is published must be checked against these records, or against
  a new manifest made the same way.

## Steps for v0.1.0-rc.1

1. **Close every blocking gate.** The release-readiness checklist must
   show every blocking gate as passed, with evidence.
2. **Check the source tree.** The tree at the release commit must be
   clean, and CI must be green on that commit.
3. **Package the tested binary at the release commit:**

   ```bash
   scripts/package-release.sh --binary target/dist/nextrs --version 0.1.0-rc.1 --out dist --toolchain 1.98.0
   ```

   Then check both of these:
   - `dist/*.manifest.json` → `binary_sha256` equals the gate record's
     checksum;
   - `dirty: false`.
4. **Smoke-test the archive:**

   ```bash
   scripts/release-smoke.sh --archive dist/nextrs-v0.1.0-rc.1-linux-x86_64.tar.gz --app <built app>
   ```

5. **Tag and push,** with authorization:

   ```bash
   git tag -a v0.1.0-rc.1 -m "NextRS v0.1.0-rc.1"
   git push origin v0.1.0-rc.1
   ```

   The tag push runs the release workflow, which provides independent
   verification.
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
