# Publishing @nextrs-sh/cli to npm (maintainers)

**Nothing here runs automatically.** `release.yml` packs, verifies and
installs the npm tarball, and uploads it as a workflow artifact. It never
publishes. Publishing is `npm-publish.yml`, dispatched by hand, from `main`,
behind the protected `npm-publish` environment.

## Rule: npm carries the GitHub release's binary

**The npm package is packed from the published GitHub release archive.** The
binary is never rebuilt for npm. Packing is deterministic, so the tarball's
SHA-256 is fixed by:

- the archive's binary;
- the archive's documents;
- `packages/cli/package.json` and `packages/cli/README.md` at the release
  commit.

That checksum is committed, and the publish workflow refuses anything else.
See `engineering/distribution.md`.

## One-time setup (owner)

Do these once, before the first publish.

1. **Claim the npm scope.** Create the npm organization `nextrs-sh`.
   - As of 2026-11-20, the registry answers `Scope not found` for both
     `@nextrs-sh` and `@nextrs`.
   - Neither scope belongs to the project yet. Owning the GitHub
     organization does not reserve the npm scope.
   - **Claim it before pushing any branch that names `@nextrs-sh/cli`.** The
     name is otherwise free for anyone to take.
   - Enable two-factor authentication for every member, and require it for
     publishing.
2. **Decide the adapter's name.** `packages/next-adapter` is named
   `@nextrs/next-adapter`, a scope the project does not control. It is
   `private: true`, so it cannot be published as it is. Before it is ever
   published, rename it into the claimed scope, for example
   `@nextrs-sh/next-adapter`.
3. **Create the protected environment.** In GitHub Settings → Environments,
   create `npm-publish`:
   - required reviewers: at least one maintainer other than the dispatcher;
   - deployment branches: `main` only.
4. **Configure trusted publishing** (OIDC) on npmjs.com for
   `@nextrs-sh/cli`:
   - repository `nextrs-sh/nextrs`;
   - workflow `npm-publish.yml`;
   - environment `npm-publish`.

   **If npm only allows this once the package exists**, the first publish
   needs a token:
   1. Create a granular access token that can publish only `@nextrs-sh/*`,
      with a short expiry.
   2. Store it as the `NPM_TOKEN` secret of the `npm-publish` environment,
      never as a repository secret.
   3. Publish once with it.
   4. Configure trusted publishing.
   5. Delete the secret and revoke the token.
5. **Restrict the package after the first publish.** In its npm settings,
   require two-factor authentication, or trusted publishing only, and
   disallow tokens.

## Steps for each version

**Prerequisites:**

- The GitHub release `v<version>` is published, as a prerelease for a
  release candidate (`release/PUBLISHING.md`).
- Its archive is the gated one.

**1. Pack locally from the published archive,** at the release commit (the
tag):

```bash
V=0.1.0-rc.1
gh release download v$V -R nextrs-sh/nextrs -D dist \
  -p "nextrs-v$V-linux-x86_64.tar.gz*" -p "nextrs-v$V-linux-x86_64.manifest.json"
scripts/package-npm.sh --archive dist/nextrs-v$V-linux-x86_64.tar.gz \
  --expect-binary-sha256 "$(cut -d' ' -f1 release/checksums/nextrs-v$V-linux-x86_64.binary.sha256)" --out dist/npm
```

The script refuses on any of these:

- a checksum differs;
- a version differs;
- the build is listed in `release/WITHDRAWN.txt`;
- `packages/cli` has uncommitted changes.

Then it runs `verify-npm-package.sh`.

**2. Install-test the tarball:**

```bash
scripts/npm-install-bundle.sh --tarball dist/npm/nextrs-sh-cli-$V.tgz \
  --manifest dist/nextrs-v$V-linux-x86_64.manifest.json --out /tmp/npm-bundle
scripts/npm-install-containers.sh --bundle /tmp/npm-bundle --results /tmp/npm-results --engine podman
scripts/npm-docker-example.sh --tarball dist/npm/nextrs-sh-cli-$V.tgz \
  --expect-sha256 "$(cut -d' ' -f1 release/checksums/nextrs-v$V-linux-x86_64.binary.sha256)" --out /tmp/docker-example --engine podman
```

**3. Commit the checksum record** with a pull request to `main`. The release
commit must contain it, so the tag is placed after this commit or moved in
the same release preparation.

```bash
cp dist/npm/nextrs-sh-cli-$V.tgz.sha256 release/checksums/
```

**4. Dry run.** Dispatch `npm-publish.yml` with `version=<V>` and
`dry_run=true`, then check the log:

- the download;
- three checksum matches: archive, binary and npm tarball;
- the `verify-npm-package.sh` result table;
- `npm publish --dry-run` listing the eight files, with tag `next`.

**5. Publish.** Dispatch it again with `dry_run=false`. A reviewer approves
the `npm-publish` environment, and the workflow runs:

```text
npm publish nextrs-sh-cli-<V>.tgz --tag next --access public --provenance
```

**6. Check the result:**

```bash
npm view @nextrs-sh/cli@$V dist.integrity dist.tarball
npm view @nextrs-sh/cli dist-tags    # next: <V>; latest must not point at a prerelease
npm audit signatures                 # in a project that installed it: provenance verified
```

Then install it from the registry in a clean directory, and run
`nextrs --version`.

**7. Update the documentation.** Only after step 6, update the README and
`engineering/installation.md` to give npm and pnpm as available. Until then
they must say the package is not published (see below).

## README text for after publication

Replace the README's Quick Start install paragraph with:

````markdown
### Install

```bash
npm install --save-dev @nextrs-sh/cli@0.1.0-rc.1   # or: pnpm add -D @nextrs-sh/cli@0.1.0-rc.1
npx next build                                      # Next.js builds with Node.js, as usual
npx nextrs start . --js-runtime embedded-v8         # NextRS serves the build
```

The package installs a precompiled binary for Linux x86_64 (glibc 2.35+):
no Rust, no compilation, no install scripts. In production, run
`node_modules/.bin/nextrs` (or copy the binary, see `examples/docker`), not
`npx`. A standalone archive is on GitHub Releases.
````

## Never

- **Never publish from a laptop.** `publishConfig.provenance` is `true`, so
  `npm publish` outside GitHub Actions fails, and that is intended.
- **Never move `latest` to a prerelease.** If it happens, run:

  ```bash
  npm dist-tag add @nextrs-sh/cli@<last stable> latest
  ```

  If no stable version exists yet, contact npm support.
- **Never `npm unpublish`** a version people may have installed. Deprecate
  it instead:

  ```bash
  npm deprecate @nextrs-sh/cli@<V> "<reason>"
  ```

  Add the binary to `release/WITHDRAWN.txt`.
- **Never publish a rebuilt binary,** or a tarball whose SHA-256 is not the
  committed record.
