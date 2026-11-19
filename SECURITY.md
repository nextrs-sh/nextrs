# Security policy

## Supported versions

NextRS is experimental. Security fixes are made on `main` and in the latest
release only.

| Version | Supported |
|---|---|
| 0.1.x (including release candidates) | Yes |
| Anything older, unreleased builds | No |

NextRS serves Next.js 16.4.x builds. It runs Next.js's own compiled
handlers, so a vulnerability in Next.js itself also applies under NextRS.

- **Keep your application's Next.js up to date.** NextRS warns at startup
  (`next-version-has-advisories`) for Next.js 16 builds older than 16.4.5.
- **Next.js vulnerabilities go upstream.** Report them to the Next.js
  project, not here.

## Reporting a vulnerability

**Please do not open a public issue.** Report the vulnerability privately
through GitHub: on the repository's **Security** tab, choose **Report a
vulnerability**.

Please include:

- the NextRS version (`nextrs --version`) and the Next.js version
  (`nextrs inspect <app>`);
- the command line used: the flags matter, especially `--net-allow`,
  `--trusted-proxy` and `--strip-forwarded-headers`;
- a minimal reproduction, ideally a small Next.js application or the
  requests involved;
- the impact you expect.

We will acknowledge the report and keep you informed. Once a fix is
released, we will publish an advisory, credited to you if you wish.

## Scope

In scope:

- the NextRS binary and its crates;
- request handling and isolation between requests;
- the outbound network policy (`--net-allow`) and SSRF protections;
- the filesystem sandbox;
- forwarded-header handling;
- image optimization;
- the caches;
- the metrics listener.

Known, documented trade-offs are not vulnerabilities in themselves. See
`engineering/security.md`:

- a V8 isolate is a memory boundary, not a security boundary;
- forwarded headers are permissive by default, as on `next start`.

A bypass of the documented protections is in scope.
