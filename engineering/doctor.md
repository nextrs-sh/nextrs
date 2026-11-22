# nextrs doctor

> **Availability: v0.1.0, from 0.1.0-rc.3.** `nextrs doctor` ships in
> `@nextrs/cli@0.1.0-rc.3` and its GitHub Release binary. The earlier
> prereleases, 0.1.0-rc.1 and 0.1.0-rc.2, do **not** include it: there
> `nextrs doctor` is an unknown subcommand. Doctor was first developed for
> v0.2.0; it moved into v0.1.0 with its checks unchanged.

`nextrs doctor` checks whether an existing Next.js application and its
production build suit NextRS, and says what to change. It reads files only.
It never runs `next build`, JavaScript or a server. It creates nothing,
changes nothing in the project and makes no network request.

- [Running it](#running-it)
- [What it checks](#what-it-checks)
- [Severity, status and exit codes](#severity-status-and-exit-codes)
- [JSON output](#json-output)
- [Common findings](#common-findings)
- [What Doctor cannot tell you](#what-doctor-cannot-tell-you)

## Running it

```bash
nextrs doctor                  # the current directory
nextrs doctor ./my-next-app    # a project directory (or its .next directory)
nextrs doctor . --json         # one JSON document on stdout, for CI
nextrs doctor . --strict       # warnings fail too
```

Through npm, with `@nextrs/cli` 0.1.0-rc.3 or later installed as a
development dependency of the application:

```bash
npx nextrs doctor
npx nextrs doctor . --json
npx nextrs doctor . --strict
pnpm exec nextrs doctor .
```

Without adding a dependency, name the package and its version:

```bash
npx --yes @nextrs/cli@0.1.0-rc.3 doctor .
```

Bare `npx nextrs` runs the `nextrs` command of the project's installed
`@nextrs/cli`. Where that package is not installed, npx looks for an npm
package called `nextrs`, which is not NextRS, so use the scoped form above.

Run `next build` first. Without a production build, Doctor still checks the
environment and the installed Next.js, but it reports the assessment as
**incomplete**.

To check a planned command line, pass the same runtime options you will
give `nextrs start`. Doctor parses them with the same code:

```bash
nextrs doctor . --js-runtime embedded-v8 --isolates 4 \
  --net-allow api.example.com --cache-dir /var/cache/my-app \
  --trusted-proxy 10.0.0.0/8 --js-env DATABASE_URL
```

| Option | Checked |
|---|---|
| `--json` | Print the report as one JSON document |
| `--strict` | Exit `1` on warnings as well as blockers |
| `--ignore-adapter-output` | Read the internal manifests, as `nextrs start --ignore-adapter-output` does |
| `--js-runtime` | Without it, Doctor assumes `embedded-v8` and reports whether the build needs it. With `none`, it reports what `nextrs start` would refuse or answer `501` |
| `--host`, `--trusted-proxy`, `--strip-forwarded-headers` | The forwarded-header policy, and whether the bind address is public |
| `--net-allow`, `--net-allow-addr`, `--net-ca-file`, `--net-*` | Parsed as `nextrs start` parses them. External rewrites are checked against the allowlist |
| `--cache-dir`, `--no-shared-cache`, `--isolates` | Whether the directory is outside the build and `node_modules` and writable (it is never created); the refusal `start` applies to `--no-shared-cache` with several isolates |
| `--image-cache-dir` | Outside the project and writable (it is never created) |
| `--js-env NAME` | Whether the variable is set where Doctor runs. Only its presence is reported |
| `--max-request-body` | Compared with `serverActions.bodySizeLimit` |

Colors are used only when stdout is a terminal. They are off with
`NO_COLOR` or `TERM=dumb`, so CI logs get plain text. Diagnostic logging
(`RUST_LOG`) goes to stderr and never mixes with the report.

The native binary must be able to run at all. Release binaries need Linux
x86_64 with glibc 2.35 or later. On other systems the binary does not load,
so Doctor cannot report anything there; the npm launcher prints the reason
instead (see [installation.md](installation.md#troubleshooting)). Node.js
is not needed to run Doctor.

## What it checks

Doctor reuses what `nextrs start` runs, so the two cannot disagree:

- **Build reading:** the adapter's inspection, the same one behind
  `nextrs inspect`.
- **Version policy:** the adapter's version functions and diagnostics.
- **Startup checks:** `nextrs start`'s own preflight. It loads the routing
  rules, route patterns, stored responses and image configuration, with no
  executor, no socket and no cache directory.
- **Runtime options:** `start`'s own flag parsers and directory checks.

The project may be someone else's checkout, `.next` included, so Doctor
does not let it point elsewhere:

- **Build metadata through a link out of `.next`.** If `.next/BUILD_ID`,
  or a link among the files directly in `.next`, `.next/server` or
  `.next/nextrs`, resolves outside `.next`, Doctor does not read the build
  (`NEXTRS-DOC-108`, a warning; the assessment is incomplete). Otherwise a
  link to `/proc/self/environ` or a credentials file would be printed as
  the build ID. `nextrs start` and `nextrs inspect` do follow such links.
- **Doctor's own reads** (`package.json`, `.env` files, file traces,
  `required-server-files.json`) are made only from regular files inside the
  project, its `.next` or its monorepo root, with a size limit. The
  installed `node_modules/next/package.json` may be linked from a package
  store; only a version-shaped value is kept from it.
- **The startup checks** load every prerendered response into memory and
  list `public/`. Doctor runs them only when each stored response is a
  regular file inside `.next`, they total at most 1 GiB, and `public/`
  has no linked directories. Otherwise it reports them as not run
  (`NEXTRS-DOC-107`, unknown).
- **Text output** shows control characters from the project (in a package
  name, a route) escaped, so they cannot drive the terminal.

| Section | Checks |
|---|---|
| Environment | Platform, C library and glibc version, NextRS version, whether the path is a Next.js project, installed Next.js against the version policy |
| Build | `.next` present and readable, the Next.js version of the build (tested / untested / unsupported / advisories), installed versus build version, `node_modules/next` present when JavaScript runs, `nextrs start`'s startup checks, stale adapter output, missing `.next/static` |
| Compatibility | App Router, Pages Router, dynamic SSR, Route Handlers, `generateStaticParams`, Server Actions, ISR, Cache Components and Partial Prerendering, custom cache handlers, the Node.js proxy, edge middleware, `skipProxyUrlNormalize`, `next.config` rules, image optimization, edge-runtime routes, interception routes, `i18n`, `instrumentation.ts` |
| Node.js APIs | Native addons, TCP clients (database, cache, mail, queue drivers) and process-spawning packages in Next's file traces of each server entry |
| Runtime configuration | Whether `--js-runtime embedded-v8` is needed, outbound network for external rewrites, remote images and `fetch`, the data and image cache directories, forwarded headers, `.env` files NextRS does not load, `--js-env` names, `NEXT_DEPLOYMENT_ID` with `runtimeServerDeploymentId`, the Server Actions key and body limit |

Each feature is classified with the statuses of the
[capability matrix](compatibility.md#capability-matrix): `supported`,
`experimental`, `partial`, `unsupported`, and `unknown` for what no suite
covers. **Experimental means it works on the fixtures, not that it is
verified for your application.** A feature is never reported as
unsupported merely because its coverage is limited.

### How Node.js APIs are judged

`next build` writes a file trace (`.nft.json`) for every server entry. A
trace lists the files Next's tracer found reachable from that entry's
server code. Client-only imports, type imports and code removed by tree
shaking are not in it. Doctor reads the traces of `server/app`,
`server/pages`, the proxy and `instrumentation`. It does not read
`next-server.js.nft.json`, which describes `next start` itself (for
example, sharp for its image optimizer). Doctor reports:

- **native addons:** `.node` files;
- **TCP clients:** `pg`, `postgres`, `mysql`, `mysql2`, `mariadb`,
  `mongodb`, `mongoose`, `ioredis`, `redis`, `@redis/client`, `memcached`,
  `nodemailer`, `tedious`, `mssql`, `oracledb`, `cassandra-driver`,
  `amqplib`, `kafkajs`, `mqtt` and `nats`;
- **process spawners:** `puppeteer`, `playwright` and their `-core`
  packages, and `@sparticuz/chromium`.

These findings are warnings, not blockers. A traced package may be loaded
only on a path that never runs, or an optional addon may be loaded inside
`try`/`catch`. Doctor does not scan source text, so it cannot tell which
built-in modules (`node:http`, `node:net`, `node:child_process`, ...) the
code calls. It always says so with an `unknown` finding.

## Severity, status and exit codes

| Severity | Meaning | Text mark |
|---|---|---|
| `error` | A confirmed blocker: `nextrs start` refuses, or part of the app is known to answer `501` or fail | `✗` |
| `warning` | Likely to matter in production; not proven to break the app | `!` |
| `unknown` | Doctor could not decide. Never a failure, even with `--strict` | `?` |
| `info` | Context or a required option, with what to do | `i` |
| `pass` | A check that passed | `✓` |

The overall status is `blocked` (any error), then `incomplete` (no
readable build), then `warnings`, then `ok`.

| Exit code | Meaning |
|---|---|
| `0` | No confirmed blockers. Warnings and unknowns may remain |
| `1` | One or more confirmed blockers; with `--strict`, warnings too |
| `2` | Invalid arguments, or the check could not run (the path does not exist or is not a directory). Nothing is printed on stdout |

**Exit code 0 does not mean the application works under NextRS.** It
means Doctor found nothing that is known not to work. Test the build with
`nextrs start` and compare its responses with `next start`.

## JSON output

`--json` prints exactly one JSON document on stdout. The output is
deterministic: no timestamps, and findings in a fixed order (by section,
then check). Absolute paths of the machine are replaced by `.` (the
project), `<workspace>` (a monorepo root) and `~` (the home directory).
Environment variable values are never included: variables are reported by
name, and `.env` files are read only for their names.

```json
{
  "schemaVersion": 1,
  "cliVersion": "0.1.0-rc.3",
  "status": "incomplete",
  "complete": false,
  "strict": false,
  "exitCode": 0,
  "project": {
    "path": ".",
    "name": "my-app",
    "nextjsProject": true,
    "buildFound": false,
    "buildId": null,
    "metadataSource": null,
    "appRouter": null,
    "pagesRouter": null
  },
  "environment": {
    "os": "linux",
    "arch": "x86_64",
    "libc": "glibc",
    "glibcVersion": "2.39",
    "nextInstalled": "16.4.6",
    "nextBuild": null,
    "nextBuildSource": null
  },
  "findings": [
    {
      "code": "NEXTRS-DOC-101",
      "severity": "warning",
      "category": "build",
      "title": "Production build missing",
      "message": "No .next/BUILD_ID: route, feature and runtime-configuration checks need a production build, so this assessment is incomplete. Doctor never runs next build itself.",
      "component": ".next",
      "suggestion": "Run next build, then repeat nextrs doctor."
    }
  ],
  "summary": { "passed": 4, "info": 2, "warnings": 1, "errors": 0, "unknown": 0 }
}
```

A finding has a `code`, `severity`, `category` (`environment`, `build`,
`compatibility`, `node-apis`, `runtime-config`), `title` and `message`. It
may also have:

- `support`: the capability status;
- `component`: a route, file or option, relative to the project;
- `suggestion`;
- `docs`: a link;
- `inspectCode`: the `nextrs inspect` diagnostic the finding reports.

Codes are stable, so CI may match on them. A schema version change means a
field was removed or its meaning changed; new optional fields keep the
version.

## Common findings

| Code | Finding | What to do |
|---|---|---|
| `NEXTRS-DOC-101` | Production build missing | Run `next build`, then Doctor again |
| `NEXTRS-DOC-102` | Build metadata could not be read | Delete `.next` and rebuild; do not copy `.next` partially |
| `NEXTRS-DOC-104` | Next.js version unsupported (error) or untested (warning) | Build with Next.js 16.4.x, the tested line |
| `NEXTRS-DOC-006` | The version has published security advisories | Upgrade to 16.4.5 or later |
| `NEXTRS-DOC-105` | `node_modules/next` missing | Install production dependencies next to the build, or ship the standalone output with its `node_modules` |
| `NEXTRS-DOC-107` | `nextrs start` would fail to start (error), or its startup checks were not run (unknown) | Fix the rule, route or file the message names; when not run, start the build with `nextrs start` |
| `NEXTRS-DOC-108` | Build metadata links outside the build | Run Doctor on a build whose `.next` holds its own files |
| `NEXTRS-DOC-207`, `-212`, `-217` | Edge-runtime Server Actions, middleware or routes | Use the Node.js runtime (`proxy.ts` instead of edge middleware) |
| `NEXTRS-DOC-218` | Interception routes (an error when the build runs JavaScript, a warning otherwise) | Not supported by the executor: every route it serves answers `501` |
| `NEXTRS-DOC-219` | `i18n` config with App Router routes | Use `next-intl`'s middleware instead of Next's `i18n` config |
| `NEXTRS-DOC-301`, `-302`, `-303` | Native addons, TCP clients, process spawners in server code | Use pure-JavaScript, WebAssembly or `fetch`-based alternatives, or move the work behind an HTTP service |
| `NEXTRS-DOC-401` | Dynamic routes require `--js-runtime embedded-v8` | Start with `--js-runtime embedded-v8` |
| `NEXTRS-DOC-404`, `-405` | Cache directory unusable | Choose a directory outside the build and `node_modules`, writable by the service user |
| `NEXTRS-DOC-406` | Forwarded headers trusted from every client on a public address | `--trusted-proxy <proxy CIDR>` behind a proxy, `--strip-forwarded-headers` without one |
| `NEXTRS-DOC-407` | `.env` files are not loaded by NextRS | Set the variables in the service environment and pass each with `--js-env NAME` |
| `NEXTRS-DOC-409` | `NEXT_DEPLOYMENT_ID` required at runtime | Set it and pass `--js-env NEXT_DEPLOYMENT_ID` |
| `NEXTRS-DOC-413`, `-414`, `-415` | External rewrites, remote images or `fetch` need `--net-allow` | Add `--net-allow <host>` per destination (`--net-allow-addr` for private addresses) |
| `NEXTRS-DOC-307` | Node.js API usage not fully analyzed | Exercise every server route under `nextrs start --js-runtime embedded-v8` and look for `NextRSUnsupportedError` |

Diagnostics of `nextrs inspect` that no check above covers are passed
through unchanged: `NEXTRS-DOC-190` (unsupported, an error), `-191`
(warning) and `-192` (info), with `inspectCode` naming them.

## What Doctor cannot tell you

- **Whether the application behaves the same as under `next start`.**
  Only the routes in NextRS's fixtures and certification applications have
  been verified.
- **Which Node.js built-ins or APIs the code calls,** or whether a traced
  package is used on a path that runs.
- **Which hosts server code fetches from.** Doctor knows only the
  destinations in the build's rewrites.
- **Runtime behavior under load:** memory, isolate stalls, timeouts.
- **Environment variables read at runtime** that are defined nowhere in
  `.env` files.
- **Builds it cannot read:** an unsupported platform, a corrupted `.next`
  (reported as a blocker), or build metadata linked from outside `.next`
  (reported as incomplete).

A `--deep` mode, with more static analysis that still runs nothing, is
future work. It is not implemented.
