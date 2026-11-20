# @nextrs/cli

The `nextrs` command for [NextRS](https://github.com/nextrs-sh/nextrs). NextRS
serves the output of an official Next.js `next build` from a native Rust
server with embedded V8.

**Next.js still builds with Node.js. NextRS runs the production web server
in Rust and embedded V8.** This package installs a precompiled binary, so you
need no Rust toolchain.

> [!WARNING]
> **Experimental prerelease.** Version 0.1.0-rc.1 is a release candidate,
> published under the `next` dist-tag. It is tested on Next.js 16.4.5 and
> 16.4.6 and on one real application, and has not run in production yet.
> Read the [known limitations](https://github.com/nextrs-sh/nextrs/blob/main/release/README.md#18-known-limitations)
> first.

## Install

**npm**

```bash
npm install --save-dev @nextrs/cli@0.1.0-rc.1
npx next build
npx nextrs start . --js-runtime embedded-v8
```

**pnpm**

```bash
pnpm add -D @nextrs/cli@0.1.0-rc.1
pnpm exec next build
pnpm exec nextrs start . --js-runtime embedded-v8
```

**Try it once, without adding a dependency** (in a project that has already
run `next build`):

```bash
npx --yes @nextrs/cli@0.1.0-rc.1 start . --js-runtime embedded-v8
```

The application is served at <http://127.0.0.1:3000>.

- **`--js-runtime embedded-v8` is required** for server rendering, route
  handlers, Server Actions, ISR and middleware. Without it NextRS serves only
  prerendered pages and static files.
- **Check the build first.** `nextrs inspect .` lists how each route will be
  served, and anything that is not supported.

## What the package contains

| File | Contents |
|---|---|
| `bin/nextrs` | The native NextRS binary: the same bytes as in the GitHub release archive |
| `BUILD-INFO.json` | Version, source commit, compiler, minimum glibc, the binary's SHA-256 |
| `LICENSE-MIT`, `LICENSE-APACHE` | NextRS's licenses |
| `THIRD-PARTY.md`, `THIRD-PARTY-LICENSES.txt` | Statically linked components, and their license and notice texts |

**What installing does not do.** The package has no install scripts and no
dependencies, so installing it:

- compiles nothing;
- downloads nothing;
- runs no code.

**How the command is linked.** `nextrs` points straight at the native
executable. No JavaScript launcher runs.

**Checking the binary:**

```bash
sha256sum node_modules/@nextrs/cli/bin/nextrs
node -p "require('@nextrs/cli/BUILD-INFO.json').binary_sha256"
```

The two values must be equal. They must also equal the `binary_sha256` of
the GitHub release manifest for the same version.

## Supported platforms

| | |
|---|---|
| Operating system | Linux |
| Architecture | x86_64 (`x64`) |
| C library | glibc 2.35 or later. Tested on Ubuntu 22.04, Ubuntu 24.04 and Debian 12. |

**Other platforms are refused.** On another OS, another CPU or musl (Alpine):

- **npm** stops the install with `EBADPLATFORM`.
- **pnpm** installs the package (it enforces `os`, `cpu` and `libc` only for
  optional dependencies), then `nextrs` refuses to start and names the
  problem.

There is no build from source behind either.

**An older glibc installs but cannot run.** The loader then exits with:

```text
nextrs: /lib/x86_64-linux-gnu/libc.so.6: version `GLIBC_2.3x' not found
```

Use a newer base system or image.

**Requirements are split by step:**

| Step | Needs |
|---|---|
| Building the application (`next build`) | Node.js and your package manager, as usual |
| Running the server | Only the binary, glibc, `libgcc_s` and `libm`: no Node.js, Rust or Cargo. The process that serves requests is `nextrs` itself. |

## Production and containers

In production, start the native binary itself, not through `npx` or
`pnpm exec`.

**Why.** `npx` and `pnpm exec` stay running as a Node.js parent process.
pnpm's `node_modules/.bin/nextrs` is a shell shim that also stays running.

```bash
exec ./node_modules/@nextrs/cli/bin/nextrs start . --host 0.0.0.0 --js-runtime embedded-v8
```

**In a container image,** copy the binary out of `node_modules` in the build
stage, and run it directly in a runtime stage that has no Node.js. A tested
multi-stage Dockerfile is in
[`examples/docker`](https://github.com/nextrs-sh/nextrs/tree/main/examples/docker).

## Without npm

The same binary is published as a standalone archive on
[GitHub Releases](https://github.com/nextrs-sh/nextrs/releases), with its
checksum and manifest. npm is only a way to install it. The server does not
depend on npm or Node.js.

## Documentation

- [Installation and operation guide](https://github.com/nextrs-sh/nextrs/blob/main/release/README.md)
- [Configuration reference](https://github.com/nextrs-sh/nextrs/blob/main/engineering/configuration.md)
- [Compatibility matrix](https://github.com/nextrs-sh/nextrs/blob/main/engineering/compatibility.md)
- [Security policy](https://github.com/nextrs-sh/nextrs/blob/main/SECURITY.md)

## License

NextRS is licensed under MIT or Apache-2.0, at your option. The binary also
contains third-party components under their own licenses, listed in
`THIRD-PARTY.md`.
