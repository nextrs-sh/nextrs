# Publishing a NextRS release (maintainers)

This is the procedure that `.github/workflows/release.yml` and
`.github/workflows/npm-publish.yml` implement, in the order used for
v0.1.0-rc.1 and v0.1.0-rc.2. **Nothing publishes automatically.** Each of
these steps needs a maintainer's decision:

- pushing a tag;
- creating the GitHub release;
- dispatching `npm-publish.yml` and approving its `npm-publish`
  environment.

Never publish to crates.io: the workspace has `publish = false`, and the
`nextrs` crate there is an unrelated project. The private
`packages/next-adapter` is not published. There is no container image.

## Rules

1. **Publish the binary that was tested.** Builds are not bit-for-bit
   reproducible: a rebuild of the same commit differs by a few kilobytes of
   code. The release is identified by the **binary's SHA-256**, built by
   `release.yml` on `main`, never on a developer machine.
2. **Publish the CI archive.** Local packaging differs in the tar header
   (GNU tar 1.35 leaves the device-number fields empty; the runner's tar
   writes zeros). The archive attached to the GitHub release is the CI
   artifact itself.
3. **npm carries the GitHub release's binary.** The npm tarball is packed
   from the published archive (`scripts/package-npm.sh`). Packing is
   deterministic, so its SHA-256 is fixed by the archive and by
   `packages/cli/package.json` and `packages/cli/README.md` at the tag.
4. **Withdrawn builds** are listed in `release/WITHDRAWN.txt`.
   `package-release.sh` refuses them, and refuses release documents that
   name them.

## Gates

`release.yml` runs them all, on every run:

- formatting, Clippy, the workspace tests and `cargo audit`;
- build and package (Ubuntu 22.04 runner: the glibc 2.35 floor);
- the runtime file audit: no access to the Cargo home, rustup, a target
  directory or the source tree at run time;
- the compatibility suites against `next start` at 1 and 4 isolates, and
  the real-world proxy applications;
- clean installs in fresh Ubuntu 22.04, Ubuntu 24.04 and Debian 12
  containers that hold only the archive and prebuilt applications;
- the npm package: packed from the archive, verified byte for byte,
  deterministic, negative tests;
- npm and pnpm installs on the same three images, and refusal on glibc 2.31
  and musl;
- the documented Docker example.

Outside CI, before publication: the application validation (Velasia
Airways in Docker, with the CI binary), recorded in the release notes.

## Steps

`V` is the version, for example `0.1.0-rc.2`.

### 1. Release preparation (pull request to `main`)

- Set the version in `Cargo.toml` (`[workspace.package]`), `Cargo.lock`
  (the five workspace crates) and `packages/cli/package.json`. The
  `version` job of `release.yml` checks they agree.
- Add `release/v$V-release-notes.md`; it is required, and goes into the
  archive as `RELEASE-NOTES.md`.
- Update the installation examples (`release/README.md`,
  `packages/cli/README.md`, `engineering/installation.md`).

The pull request runs `ci.yml` and `release.yml`. Merge it only when both
are green.

### 2. Build the candidate binary

Dispatch `release.yml` on `main` with `version=$V`. It compiles the binary
(`dist` profile, rustc pinned in the workflow) and runs every gate on it.
Record the run ID and the binary's SHA-256 (`binary_sha256` in the
manifest artifact).

Then validate that binary outside CI (the application validation), and
record the results in the release notes (pull request to `main`). Only
documents may change after the binary's commit: `package-release.sh
--built-from` refuses a change to `Cargo.toml`, `Cargo.lock` or `crates/`.

### 3. Package the gated binary at the release commit

Dispatch `release.yml` on `main` again, in **candidate mode**:

```text
version        $V
binary_run_id  <the run of step 2>
binary_sha256  <the binary's SHA-256>
```

It compiles nothing: it downloads that run's archive, checks the binary's
SHA-256, and packages it at the current `main` commit with
`--built-from <the binary's commit>`. Every gate runs again, on those exact
bytes, including the npm package. This run's archive is the one to publish.

### 4. Commit the checksum records

The archive records the commit it was packaged at, so it cannot contain
its own checksum. The records go in a **child commit that changes only
`release/checksums/`**:

- `nextrs-v$V-linux-x86_64.tar.gz.sha256`: the step 3 archive;
- `nextrs-v$V-linux-x86_64.binary.sha256`: the binary, as `nextrs`;
- `nextrs-cli-$V.tgz.sha256`: the npm tarball of the step 3 run
  (reproduce it locally from the archive with `scripts/package-npm.sh`; it
  must be identical);
- a section in `release/checksums/README.md`.

Merge it into `main` (pull request). `npm-publish.yml` accepts an archive
whose commit is an ancestor of the tag only when the commits between them
change nothing but `release/checksums/`.

### 5. Tag and GitHub release

```bash
git tag -a v$V -m "NextRS v$V" <the checksum commit>
git push origin v$V
gh release create v$V --prerelease --title "NextRS v$V" \
  --notes-file release/v$V-release-notes.md --verify-tag \
  nextrs-v$V-linux-x86_64.tar.gz nextrs-v$V-linux-x86_64.tar.gz.sha256 \
  nextrs-v$V-linux-x86_64.manifest.json
```

- The files are the step 3 artifacts, downloaded with `gh run download`.
- A release candidate is always a **prerelease**.
- The tag push runs `release.yml` once more. That run rebuilds the binary;
  its artifacts are an independent check of the pipeline and are **never**
  uploaded to the release.
- Download the published assets again and check their SHA-256 against
  `release/checksums/`.
- Never move a published tag or replace a published asset.

### 6. npm

Dispatch `npm-publish.yml` from `main`:

```text
version         $V
dry_run         true, then false
auth            trusted-publishing
promote_latest  false
```

The `npm-publish` environment's reviewer approves each run. The workflow:

1. checks out the tag and checks it is on `main`;
2. downloads the archive from the published (non-draft) GitHub release,
   and checks it against `release/checksums/`;
3. repacks the npm tarball and checks it against its record;
4. publishes under the dist-tag `next` for a prerelease (`latest` only for
   a stable version with `promote_latest`), with provenance;
5. after a real publish, checks the registry's integrity, dist-tags and
   provenance attestation.

Check the dry run's log before the real run.

## npm authentication

- **Trusted publishing (OIDC)** is the default mode for `@nextrs/cli` now
  that it exists. No token is passed. On npmjs.com, `@nextrs/cli` → Settings →
  Trusted publishing → GitHub Actions: organization `nextrs-sh`,
  repository `nextrs`, workflow `npm-publish.yml`, environment
  `npm-publish` (all case-sensitive). npm documents that a new trusted
  publisher must complete its first publish within 2 days, and it does not
  check the configuration when it is saved.
- It needs npm 11.5.1 or later and Node.js 22.14.0 or later. The workflow
  uses Node.js 24.18.0 (npm 11.16.0) and checks both.
- **`bootstrap-token`** publishes with the `npm-publish` environment's
  `NPM_TOKEN` secret. It is required for a package's very first publish
  (npm can configure trusted publishing only for an existing package), and
  was used for v0.1.0-rc.1. It is also accepted for an existing package, as
  a fallback while trusted publishing fails: the first trusted-publishing
  run, for v0.1.0-rc.2, failed with `ENEEDAUTH` (npm falls back to a token
  when the OIDC exchange fails, and none was passed), which points at the
  trusted publisher configuration on npmjs.com. The token must be a
  granular token with publish rights on `@nextrs/cli`, and the package must
  not be set to "Require two-factor authentication and disallow tokens".
  Once trusted publishing works, delete the secret, revoke the token, and
  set "disallow tokens" again.

## After publication

- `gh release view v$V`: prerelease, the tag, three assets.
- `npm view @nextrs/cli@$V version`, and `npm view @nextrs/cli dist-tags`:
  `next` is `$V`. The registry set `latest` to 0.1.0-rc.1 at the first
  publish; it moves at the first stable release.
- A clean install from the registry, in a fresh container, needs no Rust or
  Cargo: `npm install -D @nextrs/cli@$V`, `npx nextrs --version`.
