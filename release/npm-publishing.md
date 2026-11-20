# Publishing @nextrs/cli to npm (maintainers)

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

1. **The npm scope.** The package is `@nextrs/cli`, in the npm organization
   `nextrs`, which the owner created on 2026-11-20. The registry answers
   `/-/org/nextrs/package` with 200; an unclaimed name answers
   `Scope not found`.
   - The public registry cannot show who owns an organization. The owner
     confirms it on npmjs.com (Organizations → `nextrs` → Members): only
     project maintainers, each an owner or a publishing member.
   - Require two-factor authentication for every member (the
     organization's 2FA setting).
   - The GitHub organization is `nextrs-sh`; the npm organization
     `nextrs-sh` does not exist. Creating it as an empty placeholder would
     stop anyone else from using a name that looks official.
2. **The adapter's name.** `packages/next-adapter` is `@nextrs/next-adapter`,
   now in the owned scope. It stays `private: true` and is not published
   with v0.1.0-rc.1.
3. **Create the protected environment.** In GitHub Settings → Environments,
   create `npm-publish`:
   - required reviewers: at least one maintainer other than the dispatcher,
     with "prevent self-review";
   - deployment branches and tags: `main` only;
   - no repository-level `NPM_TOKEN` secret, ever.

   Also add a tag ruleset for `v*` (Settings → Rules) so that only
   maintainers can create, move or delete release tags: `npm-publish.yml`
   publishes what the tag points at.
4. **First publish: a one-time bootstrap.** npm can configure a trusted
   publisher only for a package that already exists. Its `npm trust` page
   says: "The package you're configuring must already exist on the npm
   registry". There is no pending-publisher mechanism. The first version is
   therefore published with a token, through the same workflow, so it has
   provenance and the same checks:
   1. On npmjs.com, create a **granular access token**:
      - read and write on packages, limited to the `@nextrs` scope;
      - no organization or user permissions;
      - the shortest expiry offered;
      - "bypass 2FA", because CI cannot answer a one-time password.
   2. Store it as the `NPM_TOKEN` secret **of the `npm-publish`
      environment**, not of the repository.
   3. Dispatch `npm-publish.yml` with `auth=bootstrap-token`, once with
      `dry_run=true` and then with `dry_run=false`. The workflow refuses
      `bootstrap-token` once `@nextrs/cli` exists. It passes no token at all
      with `trusted-publishing`.
   4. Right after: delete the environment secret, and revoke the token on
      npmjs.com.
5. **Restrict the package.** In `@nextrs/cli` → Settings → Publishing access,
   choose "Require two-factor authentication and disallow tokens". Trusted
   publishers keep working.
6. **Configure trusted publishing when the next version is ready.** npm
   documents that "A newly created trusted publisher configuration must
   complete its first successful publish within 2 days", or it expires.
   npm also does not check the configuration when it is saved. So add it
   only when the next version's GitHub release exists, then publish within
   two days. In `@nextrs/cli` → Settings → Trusted publishing → GitHub
   Actions, enter:
   - organization `nextrs-sh`, repository `nextrs`;
   - workflow `npm-publish.yml` (the file name only);
   - environment `npm-publish`.

   All fields are case-sensitive. `package.json`'s `repository.url` must
   name the same repository; it does. Between versions no publishing
   credential exists at all.

**npm and Node.js versions.** npm documents that trusted publishing
requires npm CLI 11.5.1 or later and Node.js 22.14.0 or later.
`npm-publish.yml` uses Node.js 24.18.0, which ships npm 11.16.0 (measured),
and its own step fails if either version is lower. Node.js 22.20.0, the
version the other CI jobs use, ships npm 10.9.3, which cannot publish with
OIDC.

**Provenance.** With trusted publishing from GitHub Actions, npm generates
provenance itself, for a public repository and package. The bootstrap
token publish gets it from `--provenance` and the job's `id-token: write`.
`publishConfig.provenance` only makes a laptop publish fail; it does not
authorize anything.

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
scripts/npm-install-bundle.sh --tarball dist/npm/nextrs-cli-$V.tgz \
  --manifest dist/nextrs-v$V-linux-x86_64.manifest.json --out /tmp/npm-bundle
scripts/npm-install-containers.sh --bundle /tmp/npm-bundle --results /tmp/npm-results --engine podman
scripts/npm-docker-example.sh --tarball dist/npm/nextrs-cli-$V.tgz \
  --expect-sha256 "$(cut -d' ' -f1 release/checksums/nextrs-v$V-linux-x86_64.binary.sha256)" --out /tmp/docker-example --engine podman
```

**3. Commit the checksum records** with a pull request to `main`, then tag.

The archive's BUILD-INFO records the commit it was packaged at (`commit`).
A commit cannot contain the checksum of an archive that names that same
commit. So the order is:

1. **Commit A:** everything that goes into the archive (release notes,
   licenses). Package the gated binary at A with `--built-from 711d77f`.
   Clean-install that archive.
2. **Commit B,** the child of A, adds only `release/checksums/`:
   - `nextrs-v$V-linux-x86_64.tar.gz.sha256` (archive at A);
   - `nextrs-v$V-linux-x86_64.binary.sha256`;
   - `nextrs-cli-$V.tgz.sha256`.
3. **The tag `v$V` goes on B.** The GitHub release carries the archive
   packaged at A.

`npm-publish.yml` accepts an archive whose `commit` is the tag, or an
ancestor of it from which only `release/checksums/` changed. It refuses
anything else.

```bash
cp dist/nextrs-v$V-linux-x86_64.tar.gz.sha256 release/checksums/
printf '%s  nextrs\n' "$(node -p "require('./dist/nextrs-v$V-linux-x86_64.manifest.json').binary_sha256")" \
  > release/checksums/nextrs-v$V-linux-x86_64.binary.sha256
cp dist/npm/nextrs-cli-$V.tgz.sha256 release/checksums/
```

**4. Dry run.** Dispatch `npm-publish.yml` from `main` with these inputs:

- `version=<V>`;
- `dry_run=true`;
- `auth=bootstrap-token` for v0.1.0-rc.1 (the first version), or
  `trusted-publishing` for later versions.

Then check the log:

- the tag is on `main`;
- node and npm meet the trusted-publishing minimum;
- the release is published (not a draft) and marked prerelease;
- the archive, binary and npm tarball match the committed records;
- the archive's commit is the tag or its checksum-only parent;
- the `verify-npm-package.sh` result table;
- `npm publish --dry-run` lists the eight files, with tag `next`.

**5. Publish.** Dispatch it again with `dry_run=false`. A reviewer approves
the `npm-publish` environment, and the workflow runs:

```text
npm publish nextrs-cli-<V>.tgz --tag next --access public --provenance
```

It then waits for the registry and checks three things:

- the version's `dist.integrity` equals the packed tarball's;
- `next` points at `<V>`;
- a provenance attestation exists.

**`latest` on the first publish.** The workflow never asks for `latest` for
a prerelease, but npm's documentation does not say what the registry does
to `latest` at a package's first publish. If the registry points `latest`
at 0.1.0-rc.1 because it is the only version, the workflow warns. `latest`
then moves with the first stable release (`promote_latest=true`), and the
README pins the exact version until then.

**6. Check the result** from a clean machine:

```bash
npm view @nextrs/cli@$V dist.integrity dist.tarball
npm view @nextrs/cli dist-tags    # next: <V>
mkdir /tmp/t && cd /tmp/t && npm init -y && npm install -D @nextrs/cli@$V
npm audit signatures              # "verified registry signatures" and "verified attestations"
sha256sum node_modules/@nextrs/cli/native/nextrs   # the binary record
npx nextrs --version
```

Repeat the install with `pnpm add -D @nextrs/cli@$V`. Then run
`scripts/npm-install-containers.sh` against the registry tarball
(`npm pack @nextrs/cli@$V`). Its SHA-256 must be the committed
`nextrs-cli-$V.tgz.sha256`.

**7. Update the documentation.** Only after step 6, update the README and
`engineering/installation.md` to give npm and pnpm as available. Until then
they must say the package is not published (see below).

## README text for after publication

Replace the README's Quick Start install paragraph with:

````markdown
### Install

With npm:

```bash
npm install -D @nextrs/cli@0.1.0-rc.1
npx next build
npx nextrs start . --js-runtime embedded-v8
```

With pnpm:

```bash
pnpm add -D @nextrs/cli@0.1.0-rc.1
pnpm exec next build
pnpm exec nextrs start . --js-runtime embedded-v8
```

**What you need.**

- Node.js is still needed, for the official `next build`.
- Rust and Cargo are not: the package installs a precompiled NextRS
  binary, with no compilation, install scripts or downloads.
- Linux x86_64 with glibc 2.35 or later, for example Ubuntu 22.04+ or
  Debian 12+.
- Not supported: Alpine/musl, arm64, macOS and Windows.

**In production**, start `node_modules/.bin/nextrs start …`, or copy the
binary into the image (see `examples/docker`). Don't use `npx`,
`pnpm exec` or `npm start`: those keep a wrapper process, and a SIGTERM
sent only to the wrapper can leave the server running. A standalone archive
is on GitHub Releases.
````

## Never

- **Never publish from a laptop.** `publishConfig.provenance` is `true`, so
  `npm publish` outside GitHub Actions fails, and that is intended.
- **Never move `latest` to a prerelease.** If it happens, run:

  ```bash
  npm dist-tag add @nextrs/cli@<last stable> latest
  ```

  If no stable version exists yet, contact npm support.
- **Never `npm unpublish`** a version people may have installed. Deprecate
  it instead:

  ```bash
  npm deprecate @nextrs/cli@<V> "<reason>"
  ```

  Add the binary to `release/WITHDRAWN.txt`.
- **Never publish a rebuilt binary,** or a tarball whose SHA-256 is not the
  committed record.
