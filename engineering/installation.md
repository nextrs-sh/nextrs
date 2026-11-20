# Installing NextRS

**Next.js still builds with Node.js. NextRS runs the production web server
in Rust and embedded V8.** Installing NextRS never needs Rust, Cargo or a
compiler: every channel delivers the same precompiled binary.

> [!IMPORTANT]
> **Availability (2026-11-20).** Nothing is published yet:
>
> - **npm:** `@nextrs-sh/cli` is not on the npm registry. The commands in
>   [Install with npm](#install-with-npm) and
>   [Install with pnpm](#install-with-pnpm) work only with a locally packed
>   tarball until it is.
> - **GitHub:** the v0.1.0-rc.1 release, with its archive, is not published.
> - **Today:** [build from source](../README.md#quick-start), or pack the
>   binary of a release workflow run, as described in
>   `release/npm-publishing.md`.

## Contents

1. [Requirements](#requirements)
2. [Supported platforms](#supported-platforms)
3. [Install with npm](#install-with-npm)
4. [Install with pnpm](#install-with-pnpm)
5. [Try it without installing](#try-it-without-installing)
6. [Install the standalone binary](#install-the-standalone-binary)
7. [npm or standalone: differences](#npm-or-standalone-differences)
8. [Running in production](#running-in-production)
9. [Docker](#docker)
10. [Verifying what you installed](#verifying-what-you-installed)
11. [Troubleshooting](#troubleshooting)

## Requirements

| Step | Needs |
|---|---|
| **Building the application** (`next build`) | Node.js and a package manager, as for any Next.js project. With the npm package, the same package manager installs NextRS. |
| **Running the server** | The `nextrs` binary, and a Linux x86_64 system with glibc 2.35 or later, `libgcc_s.so.1` and `libm.so.6` (present on every glibc distribution). No Node.js, Rust, Cargo or CA bundle: TLS uses NextRS's built-in Mozilla roots. |

**Two other things are unchanged:**

- **Supported builds.** NextRS serves official `next build` output of
  Next.js 16.4.x, tested on 16.4.6 and 16.4.5. See
  [compatibility](compatibility.md).
- **Your backends.** A database, an API or an authentication service written
  for Node.js keeps running on Node.js. NextRS replaces only the Next.js web
  server process.

## Supported platforms

| | Supported | Not supported (refused) |
|---|---|---|
| OS | Linux | macOS, Windows |
| CPU | x86_64 (`x64`) | arm64 and every other CPU |
| C library | glibc 2.35+ (tested on Ubuntu 22.04, Ubuntu 24.04 and Debian 12) | musl (Alpine), and glibc older than 2.35 (Ubuntu 20.04, Debian 11) |

**Other platforms are refused, at install or at the first run.**

- **Another OS, CPU or libc with npm:** the install stops with
  `EBADPLATFORM`.
- **The same with pnpm:** pnpm installs the package, because it checks these
  fields only for optional dependencies. `nextrs` then refuses to start and
  names the problem.
- **glibc older than 2.35:** this cannot be declared to npm. The package
  installs, then `nextrs` exits with a message naming the requirement.

**Nothing is ever compiled as a fallback.**

## Install with npm

Add NextRS as a development dependency of the application:

```bash
npm install --save-dev @nextrs-sh/cli@0.1.0-rc.1
npx next build
npx nextrs start . --js-runtime embedded-v8
```

**What installing does.** It downloads one package, about 29 MB compressed
and 90 MB on disk. The package has no dependencies and no install scripts,
so the install is the same with `--ignore-scripts`.

**`--js-runtime embedded-v8` is required** for anything dynamic: server
rendering, route handlers, Server Actions, ISR and middleware. Without it,
NextRS serves only prerendered pages and static files. A build that needs
JavaScript is refused at startup.

**Check the build before serving it:**

```bash
npx nextrs inspect .
```

It reports how each route will be served, and anything unsupported.

**Using an npm script.** For `npm run`, add:

```json
{ "scripts": { "serve:nextrs": "nextrs start . --js-runtime embedded-v8" } }
```

npm scripts and `npx` are fine during development. For a production
process, see [Running in production](#running-in-production).

## Install with pnpm

```bash
pnpm add -D @nextrs-sh/cli@0.1.0-rc.1
pnpm exec next build
pnpm exec nextrs start . --js-runtime embedded-v8
```

**pnpm 10 is fine as it is.** It does not run dependency build scripts unless
they are allowed. NextRS has none, so nothing needs to be allowed.

## Try it without installing

In an application that has already run `next build`:

```bash
npx --yes @nextrs-sh/cli@0.1.0-rc.1 start . --js-runtime embedded-v8
pnpm --package=@nextrs-sh/cli@0.1.0-rc.1 dlx nextrs start . --js-runtime embedded-v8
```

## Install the standalone binary

Without npm, download the archive from GitHub Releases, verify it and put
the binary on your `PATH`:

```bash
V=0.1.0-rc.1
curl -LO https://github.com/nextrs-sh/nextrs/releases/download/v$V/nextrs-v$V-linux-x86_64.tar.gz
curl -LO https://github.com/nextrs-sh/nextrs/releases/download/v$V/nextrs-v$V-linux-x86_64.tar.gz.sha256
sha256sum -c nextrs-v$V-linux-x86_64.tar.gz.sha256
tar -xzf nextrs-v$V-linux-x86_64.tar.gz
sudo install -m 0755 nextrs-v$V-linux-x86_64/nextrs /usr/local/bin/nextrs
nextrs --version
```

The full guide, covering operation, caches, limits and the archive layout,
is [`release/README.md`](../release/README.md).

## npm or standalone: differences

| | npm / pnpm | Standalone archive |
|---|---|---|
| Binary | the same bytes (SHA-256 `binary_sha256` in both manifests) | the same bytes |
| Installs with | your project's package manager; version pinned in the lockfile with an integrity hash | curl, sha256sum, tar |
| Location | `node_modules/@nextrs-sh/cli/native/nextrs`, run through `node_modules/.bin/nextrs` | where you install it, e.g. `/usr/local/bin/nextrs` |
| Startup | a POSIX `sh` launcher checks the platform, then `exec`s the binary (a few ms) | the binary directly |
| Checksum record | `BUILD-INFO.json` in the package; the lockfile's integrity | `.sha256` file; `BUILD-INFO.json` |
| Includes | binary, launcher, `BUILD-INFO.json`, licenses, third-party notices | the same, plus this guide and the release notes |
| Needs Node.js to run | no | no |

## Running in production

**The serving process is always the native `nextrs` binary.** What differs
is whether anything stays between it and the process manager:

| How you start it | Processes | Signals (SIGTERM from systemd, Kubernetes, `docker stop`) |
|---|---|---|
| `node_modules/.bin/nextrs start …` (npm or pnpm) | `nextrs` only: the launcher replaces itself | reach the server; graceful shutdown, exit 0 |
| `node_modules/@nextrs-sh/cli/native/nextrs start …`, or a copied binary | `nextrs` only | reach the server; graceful shutdown, exit 0 |
| `npx nextrs start …`, `npm start` | npm (Node.js) → `sh -c` → `nextrs` | **not reliable.** On Debian and Ubuntu, `/bin/sh` (dash) dies on SIGTERM without passing it on. npm exits 143, and the server keeps running, orphaned. |
| `pnpm exec nextrs start …` | pnpm (Node.js) → `nextrs` | reach the server; pnpm reports 143 |

**What to use:**

- **Production and process managers:** `node_modules/.bin/nextrs`, or the
  native binary itself, as the process the manager starts.
- **Shell wrappers:** end them with `exec`.
- **Terminals:** Ctrl-C signals the whole process group, so `npx` and
  `pnpm exec` stop cleanly there.

For example:

```bash
exec ./node_modules/.bin/nextrs start . --host 0.0.0.0 --port 3000 --js-runtime embedded-v8 \
  --trusted-proxy 10.0.0.0/8 --cache-dir /var/cache/nextrs/data
```

Configuration, the network allowlist, forwarded headers, caches, health
endpoints and shutdown are in [`release/README.md`](../release/README.md)
and [configuration.md](configuration.md).

## Docker

[`examples/docker/Dockerfile`](../examples/docker/Dockerfile) is a tested
multi-stage build.

**Build stage** (`node:22-bookworm-slim`):

1. `npm ci` installs the application's dependencies, with `@nextrs-sh/cli`
   as a devDependency.
2. `next build` builds with `output: "standalone"`.
3. The installed binary is checked against `BUILD-INFO.json`.

**Runtime stage** (`debian:12-slim`). It contains:

- `/usr/local/bin/nextrs`;
- the standalone output, with `.next/static` and `public`;
- the license files.

It has no Node.js, npm or Rust. NextRS is PID 1 and runs as an unprivileged
user.

```bash
docker build -t my-app .
docker run --rm -p 3000:3000 my-app
```

**Edit the `CMD` for your application:**

- `--js-env NAME` for each environment variable its JavaScript reads;
- `--net-allow` for each outbound destination;
- `--trusted-proxy` behind a load balancer.

**Base images.** The runtime base image must have glibc 2.35 or later and
`libgcc_s`:

- **Tested with this Dockerfile:** `debian:12-slim`.
- **Tested for the binary itself** (clean-install and npm tests):
  `ubuntu:22.04` and `ubuntu:24.04`.
- **Untested:** distroless and `scratch` images.
- **Fail:** Alpine (musl).

## Verifying what you installed

```bash
sha256sum node_modules/@nextrs-sh/cli/native/nextrs
node -p "require('@nextrs-sh/cli/BUILD-INFO.json').binary_sha256"
node -p "require('@nextrs-sh/cli/package.json').nextrs"
```

**The two checksums must be equal.** They must also equal `binary_sha256`
in the GitHub release manifest of the same version. The `nextrs` block of
`package.json` records:

- the binary commit and the release commit;
- the archive checksum;
- the minimum glibc;
- the compiler.

**Integrity at install.** npm and pnpm record each package's `sha512`
integrity in the lockfile. A tarball that changed afterwards fails
`npm ci` with `EINTEGRITY`, and fails pnpm with
`ERR_PNPM_TARBALL_INTEGRITY`.

## Troubleshooting

| Message | Cause | Fix |
|---|---|---|
| `npm error code EBADPLATFORM … Unsupported platform for @nextrs-sh/cli` | not Linux x64 glibc | Use a supported system or image. There is no build from source behind the package. |
| `nextrs: this package holds a Linux x86_64 binary; this system is '…'` | another OS or CPU (pnpm installs it; npm would refuse) | The same. |
| `nextrs: NextRS needs glibc 2.35 or later; this system has glibc 2.31` | the distribution is too old (Ubuntu 20.04, Debian 11) | Ubuntu 22.04+, Debian 12+, or a newer base image |
| `nextrs: this system uses musl libc (for example Alpine Linux)` | Alpine, after a forced install | A glibc image such as `debian:12-slim` |
| `nextrs: the native binary is missing: …` | the package directory was modified or pruned | `rm -rf node_modules && npm ci` (or `pnpm install --force`) |
| `nextrs: the native binary is not executable: …` | the file mode was lost, for example after copying through a tool that drops modes | Reinstall, or `chmod 0755` the path shown |
| `error while loading shared libraries: libgcc_s.so.1` | a stripped image without the GCC runtime library | Use a base image with `libgcc_s` (any Debian or Ubuntu image) |
| `npm error code EINTEGRITY` / `ERR_PNPM_TARBALL_INTEGRITY` | the downloaded tarball is not the one the lockfile recorded | Do not override. Find out why the tarball changed. |
| The server keeps running after the process manager stopped `npx` | `npx` / `npm start` under a process manager | Start `node_modules/.bin/nextrs` or the native binary directly (see above) |

For runtime errors, such as startup refusals, `501`, `fetch` failures or
proxies, see [`release/README.md`, Troubleshooting](../release/README.md#16-troubleshooting).
