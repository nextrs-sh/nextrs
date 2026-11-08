# Security

> [!CAUTION]
> **NextRS is experimental software. Do not expose it to untrusted traffic.**

This page is the full security notice. It describes what NextRS isolates,
what it does not, and what an operator must configure.

- [Network exposure](#network-exposure)
- [JavaScript execution and isolation](#javascript-execution-and-isolation)
- [Outbound network policy](#outbound-network-policy)
- [Cache contents](#cache-contents)
- [Resource limits](#resource-limits)
- [Server Actions](#server-actions)
- [Reverse proxies and forwarded headers](#reverse-proxies-and-forwarded-headers)
- [Proxies are not authorization](#proxies-are-not-authorization)
- [Metrics endpoint](#metrics-endpoint)
- [Unsupported-feature bypass](#unsupported-feature-bypass)

## Network exposure

`nextrs start` binds to `127.0.0.1` unless you pass `--host`, and it logs a
warning for any other address.

## JavaScript execution and isolation

- **The JavaScript engines are not hardened.** This applies to the embedded
  V8 executor and to the engine experiments. Do not put either in front of
  untrusted traffic.
- **A V8 isolate is a memory boundary, not a security boundary.** Run one
  application per process. Running untrusted applications side by side
  would also require operating-system sandboxing.
- **The executor's sandbox is narrow.** JavaScript can read only `.next` and
  `node_modules`. It cannot write files or spawn processes. It sees only
  `NODE_ENV=production`, the variables passed with `--js-env NAME`, and
  `NEXT_SERVER_ACTIONS_ENCRYPTION_KEY` when it is set (as `next start`
  reads it).
- **`--v8-flags` is for experiments.** Some V8 flags weaken the sandbox.

## Outbound network policy

Network access is opt-in and checked in Rust. Without `--net-allow`, `fetch`
fails and external rewrites answer `502`. With it, only the listed origins
are reachable, and only at public addresses unless `--net-allow-addr` names a
range. Addresses are checked after DNS resolution, redirects are re-checked,
and proxy environment variables are ignored.

Allow only what the application needs: an allowed destination receives
whatever the application sends it.

## Cache contents

The data cache is written by Rust, not by JavaScript. JavaScript passes cache
keys and values; file names are hashes of the keys, files are private to the
user, and the directory must be outside the paths JavaScript can read.
Cached responses can contain user data, so treat `--cache-dir` and
`--image-cache-dir` like application data.

## Resource limits

CPU and heap limits are per isolate: there is no per-request CPU limit. A
request that keeps the CPU busy past `--js-stall-timeout`, or exhausts the
heap, terminates its whole isolate and fails every request in it. Run more
than one isolate to contain this.

Admission, queue, body, response-buffer, connection and image limits are
listed in [configuration.md](configuration.md). Image work is bounded by
memory as well as by count: each transformation reserves its estimated
memory before it starts, and source bytes held in memory have their own
budget.

`--net-ca-file` adds trust anchors for outbound connections; it never turns
certificate verification off, and it applies only to destinations
`--net-allow` already allows.

## Server Actions

- **Server Actions are public endpoints.** Anyone who has a page can call its
  actions, so authorization belongs in each action, as with `next start`.
- **Next's own checks run unchanged:** the `Origin` comparison (with
  `serverActions.allowedOrigins`), body limits and closure decryption. That
  comparison trusts `X-Forwarded-Host`; see
  [Reverse proxies](#reverse-proxies-and-forwarded-headers).
- **Self-requests stay in-process.** Next's requests to its own server
  (redirect payloads, forwarding an action to another page) are served
  inside the process and never use the network.
- **No automatic retries.** A request that reached JavaScript is never
  retried: if its isolate fails, the client gets a `502` and cannot know
  whether the mutation happened. Do not retry actions on `502` or `504`
  automatically.

## Reverse proxies and forwarded headers

Next.js trusts `X-Forwarded-Host` and `X-Forwarded-Proto` for the Server
Action origin check, redirects and absolute URLs. Without configuration,
NextRS passes these headers through from every client, as `next start` does.

- Behind a reverse proxy, make the proxy overwrite them, or list it with
  `--trusted-proxy CIDR` so they are removed from every other connection.
- With no proxy in front, use `--strip-forwarded-headers`.
- With a chain of proxies, list the one that connects to NextRS.

`--trusted-proxy` never makes an `Origin` acceptable that Next would refuse;
it only decides whose forwarded headers Next sees. Next's internal headers
are removed from every client request regardless.

## Proxies are not authorization

NextRS runs `proxy.ts` exactly where and as `next start` does, including
Next's matcher semantics, so a matcher that misses a path misses it here
too. Check the session in Server Actions and route handlers as well.

## Metrics endpoint

`--metrics-addr` exposes counters, per-isolate state and the first line of
the last isolate failure (which can contain application error text). No
request paths, headers, URLs or cache keys are used as labels. Keep it on
loopback.

## Unsupported-feature bypass

`--allow-unsupported` removes a safety check. It lets NextRS start a build
that uses features it cannot apply (edge middleware, Next's `i18n` config,
Pages Router output from a deployment adapter). It does **not** implement
them. A build with a proxy
started without the executor answers `501` for every request the proxy
matches, rather than skipping the proxy.

