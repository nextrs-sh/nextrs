# Distribution: one binary, two channels

NextRS ships as a single native executable, `nextrs`. A release reaches users
through two channels:

| Channel | What the user gets | Needs |
|---|---|---|
| GitHub release archive `nextrs-v<version>-linux-x86_64.tar.gz` | the binary, `BUILD-INFO.json`, licenses, notices, install guide | curl, tar, sha256sum |
| npm package `@nextrs/cli` | the same binary, plus a POSIX launcher for `node_modules/.bin` | Node.js and npm or pnpm (already there to run `next build`) |

**npm is only a way to install the binary.** The server never runs on
Node.js, and it does not need npm at run time.

## The exact-binary rule

Builds are not bit-for-bit reproducible: two builds of the same commit
differ by a few kilobytes of code. So both channels
must carry the bytes that passed the release gates, never a rebuild.

1. The release workflow (`.github/workflows/release.yml`, run by pushing a
   `v*` tag on main) builds the binary once (`cargo build --profile dist`).
2. Its SHA-256 is recorded in the archive's manifest.
3. `scripts/package-release.sh` packages that binary into the GitHub archive.
4. `scripts/package-npm.sh` extracts the same archive and copies its binary,
   `BUILD-INFO.json`, licenses and notices into the npm package, byte for
   byte. It never reads a Cargo target directory.
5. `scripts/verify-npm-package.sh --archive` compares the packed binary with
   the archive's (`cmp`), and both checksums with the manifest.
6. The same run attaches the archive to the GitHub release and publishes
   the tarball to npm as `latest`.

```text
cargo build (once) ──► binary 44f04e58…
                            │
          package-release.sh▼
              nextrs-v<v>-linux-x86_64.tar.gz ──────────► GitHub release
                            │ (extract, copy bytes)
              package-npm.sh▼
              nextrs-cli-<v>.tgz ── verify (cmp) ────► npm registry (latest)
```

**The tarball is deterministic.** `npm pack` writes fixed timestamps and
modes, and `package-npm.sh` stages only the archive's bytes plus two
committed files: `packages/cli/package.json` and `packages/cli/README.md`.
Packing the same archive at the same commit always gives the same tarball
SHA-256.

## Package layout

```text
package/
  package.json                 name, version, bin, os/cpu/libc, no scripts, no dependencies,
                               "nextrs": { binarySha256, binaryCommit, releaseCommit,
                                           archiveSha256, minGlibc, rustc, target }
  README.md                    packages/cli/README.md
  bin/nextrs                   the launcher (#!/bin/sh, 1.9 KB)
  native/nextrs                the release binary (identical to the archive's)
  BUILD-INFO.json              from the archive
  LICENSE-MIT, LICENSE-APACHE  from the archive
  THIRD-PARTY.md, THIRD-PARTY-LICENSES.txt   from the archive
```

**What the metadata declares:**

- **No `scripts` and no dependencies.** Installing runs no code from this
  package, compiles nothing and downloads nothing. `package-npm.sh` and
  `verify-npm-package.sh` both refuse a `scripts`, `gypfile` or dependency
  field.
- **`"os": ["linux"]`, `"cpu": ["x64"]`, `"libc": ["glibc"]`.** npm refuses
  to install the package elsewhere (`EBADPLATFORM`).
  - pnpm 10.23.0 enforces these fields only for optional dependencies. It
    installed the package on Alpine (musl) and under a simulated arm64 or
    darwin without an error.
  - There, the launcher refuses at the first run instead.
- **The glibc version cannot be declared** in `package.json`. The launcher
  checks it at run time.
- **`publishConfig`:** `access: public`, `tag: next` and `provenance: true`.
  A prerelease must never become `latest`, and `verify-npm-package.sh`
  rejects a prerelease that names `latest`.

## Why a launcher, and why it is a shell script

The preferred design was to point `bin` straight at the ELF binary, and npm
handles that well: `node_modules/.bin/nextrs` becomes a symlink to it. pnpm
does not.

**pnpm keeps a parent shell.** pnpm writes `.bin` entries as shell shims. For
a file without a `#!` line, the shim runs it as a child and then runs
`exit $?`. The shim's shell therefore stays as the server's parent.
Measured with pnpm 10.23.0: SIGTERM sent to that shell killed the shell and
left the server running, orphaned. Sending SIGINT to it did nothing until the
server exited. Process managers and container runtimes signal one PID, so
this is a correctness problem, not a cosmetic one.

**With a `#!` line, pnpm's shim `exec`s the script.** `bin/nextrs` is
therefore a POSIX `sh` script that ends in `exec "$native" "$@"`. Every route
collapses to one process, the native server:

| Route | Process that serves requests | Its parent |
|---|---|---|
| npm `node_modules/.bin/nextrs` (symlink → launcher → exec) | `native/nextrs` | the caller |
| pnpm `node_modules/.bin/nextrs` (shim exec → launcher → exec) | `native/nextrs` | the caller |
| `npx nextrs` | `native/nextrs` | `sh -c` (dash) or npm (bash), then npm (Node.js) |
| `pnpm exec nextrs` | `native/nextrs` | pnpm (Node.js) |

**Before the `exec`, the launcher checks the platform.** It costs three small
processes (`readlink`, `uname`, `getconf`), a few milliseconds per start, and
gives a diagnosis where the loader would print a bare error:

| Condition | Launcher message (exit status) |
|---|---|
| Not Linux x86_64 (forced install) | `this package holds a Linux x86_64 binary; this system is '…'` (1) |
| glibc older than 2.35 | `NextRS needs glibc 2.35 or later; this system has glibc 2.31` (1) |
| musl libc (forced install) | `this system uses musl libc (for example Alpine Linux); NextRS needs glibc 2.35 or later` (1) |
| `native/nextrs` missing | `the native binary is missing: … Reinstall the package` (127) |
| `native/nextrs` not executable | `the native binary is not executable: … chmod 0755` (126) |

**There is never a fallback to building from source.** When a check passes,
the exit status, signals and file descriptors are the native server's own.

**Production should not start NextRS through `npx`, `pnpm exec` or
`npm start`.** Each leaves a Node.js or shell parent in place. Measured
inside Debian 12, where `/bin/sh` is dash:

- **`npx` passes SIGTERM to `sh -c`, which dies.** npm exits 143 and the
  server keeps running, orphaned.
- **Ctrl-C in a terminal is not affected.** It signals the whole process
  group, so the server stops gracefully.

The supported production forms are `node_modules/.bin/nextrs` and the native
path itself. In a container, the native binary is copied out of the package
(`examples/docker`).

## Verification available to users

- **Integrity:**
  - npm and pnpm lockfiles record the tarball's `sha512` integrity.
  - A changed tarball fails `npm ci` with `EINTEGRITY` (with a cold cache)
    or pnpm with `ERR_PNPM_TARBALL_INTEGRITY`.
  - With a warm cache, npm installed the original bytes it had verified
    before.
- **Binary identity:** `sha256sum node_modules/@nextrs/cli/native/nextrs`
  must equal `BUILD-INFO.json` → `binary_sha256` and the GitHub release
  manifest's `binary_sha256`.
- **Provenance:** published from `release.yml` with `--provenance`, so
  the npm package page links the tarball to the workflow run and commit.

## Tests

| Script | What it proves |
|---|---|
| `scripts/verify-npm-package.sh` | allowlisted contents only, no secrets, modes, metadata, binary bytes equal to the archive's, versions, ELF and glibc floor |
| `tests/npm/packaging-negative.sh` | 18 refusals: archive or manifest checksum, expected checksum, binary or template or BUILD-INFO version mismatch, withdrawn build, missing notices, lifecycle script (packing); extra file, secret, mode 0644, version mismatch, install script, changed bytes (checksum and archive comparison), widened `cpu`, launcher without `exec` (verification) |
| `scripts/npm-install-containers.sh` | npm and pnpm on Ubuntu 22.04, Ubuntu 24.04 and Debian 12 with no Rust: offline install, `next build`, serving, SSR, RSC, Server Actions, static files, process model, signals, exit codes, lifecycle scripts off, one-off `npx` and `pnpm dlx`, tampered or truncated or broken packages, another CPU or OS (simulated), libgcc_s missing; glibc 2.31 (Ubuntu 20.04) and musl (Alpine) for real |
| `scripts/npm-docker-example.sh` | the documented multi-stage Dockerfile: no Node.js or Rust in the runtime image, PID 1 is the release binary, HTTP checks, graceful `docker stop` |

`release.yml` runs `verify-npm-package.sh` (through `package-npm.sh`) on
every release. The other scripts are not run in CI; run them locally when
the packaging changes.

## More platforms later

Once more targets are built and tested, the package becomes a small
`@nextrs/cli` with one optional dependency per platform. Each platform
package carries one binary and its own `os`, `cpu` and `libc`. For example:

```text
@nextrs/cli               launcher, resolves the platform package
@nextrs/linux-x64-gnu     this release's binary
@nextrs/linux-arm64-gnu   (not built)
@nextrs/darwin-arm64      (not built)
```

These names are an architecture sketch: nothing is reserved, and no such
binary exists. Version 0.1.0-rc.1 ships the single-package form, because it
has one supported platform.
