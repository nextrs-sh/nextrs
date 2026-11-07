#!/usr/bin/env python3
"""Phase 2C benchmarks: authentication libraries and the other Phase 2C
fixtures against `next start` and the Phase 2B binary (stdlib only; load: ab).

    python3 benchmarks/phase2c_bench.py --nextrs-2b target/baseline/nextrs-2b --out benchmarks/results

Servers per fixture (each with the test upstream and the OpenID provider stub):

  next-start   `next start -H 0.0.0.0`
  2b           the Phase 2B binary (--nextrs-2b), 1 isolate: measured only on
               paths where it answers as next start does (it cannot verify
               asymmetric signatures, so the JWKS and Auth.js paths are
               skipped by the status check)
  2c           this tree, 1 isolate
  2c-i4        this tree, 4 isolates

Paths:

  fixtures/next-auth-proxy      HS256 session (jose in the proxy) on a prerendered page
  fixtures/next-jwks-proxy      bearer tokens verified against the JWKS (RS256, ES256,
                                EdDSA; jose's cache refetches every 1.5 s per isolate),
                                an ES256 session page, protected ISR, a protected
                                Server Action (one ledger write each), an external
                                rewrite behind the proxy
  fixtures/next-authjs          an Auth.js session (JWE, HKDF) on a route handler
                                (`auth()` wrapper) and on a page behind the proxy
  fixtures/next-intl-proxy      the as-needed build: an unprefixed path (rewrite) and
                                a prefixed one
  fixtures/next-platform-proxy  a proxy with an outbound fetch

Each path's status is checked against next start before it is measured;
paths are only compared with equal status. The fixtures must be built
(the jwks one against --upstream-port, for its external rewrite; the intl
one with NEXTRS_VARIANT=as-needed). NextRS's /metrics.json gives executor
requests per HTTP request, queue and execution time and fetch latency.
"""

import argparse
import base64
import json
import os
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from bench import ROOT  # noqa: E402
from phase1c_bench import free  # noqa: E402
from phase2b_bench import action_body, session_token  # noqa: E402
import phase2b_bench  # noqa: E402


def node(script, *argv, env=None):
    return subprocess.run(["node", "--input-type=module", "-e", script, *argv], check=True, capture_output=True, text=True, env=env).stdout.strip()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--out", default="benchmarks/results")
    parser.add_argument("--nextrs-2b", default=None)
    parser.add_argument("--concurrency", type=int, default=32)
    parser.add_argument("--requests", type=int, default=4000)
    parser.add_argument("--repeats", type=int, default=2)
    parser.add_argument("--upstream-port", type=int, default=4986)
    parser.add_argument("--oidc-port", type=int, default=4987)
    parser.add_argument("--only-fixture", nargs="*", default=None)
    args = parser.parse_args()

    upstream = f"http://127.0.0.1:{args.upstream_port}"
    issuer = f"http://127.0.0.1:{args.oidc_port}"
    for port in (args.upstream_port, args.oidc_port):
        if not free(port):
            sys.exit(f"port {port} is in use")
    secret = base64.urlsafe_b64encode(os.urandom(24)).decode()
    auth_secret = base64.urlsafe_b64encode(os.urandom(32)).decode()
    client_secret = base64.urlsafe_b64encode(os.urandom(18)).decode()
    app_keys = subprocess.run(["node", str(ROOT / "tests/compat/app-keys.mjs")], check=True, capture_output=True, text=True).stdout.strip()
    procs = [
        subprocess.Popen(["node", str(ROOT / "tests/compat/upstream.mjs"), "--port", str(args.upstream_port)],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True),
        subprocess.Popen(["node", str(ROOT / "tests/compat/oidc-stub.mjs"), "--port", str(args.oidc_port), "--client-id", "nextrs-test-client", "--client-secret", client_secret],
                         stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True),
    ]
    time.sleep(1)
    env = {
        "PATH": os.environ["PATH"], "HOME": os.environ["HOME"], "NEXT_TELEMETRY_DISABLED": "1",
        "NEXTRS_UPSTREAM": upstream, "SESSION_SECRET": secret, "NEXTRS_CLOSED_PORT": "9", "NEXTRS_UNLISTED_PORT": "10",
        "OIDC_ISSUER": issuer, "APP_KEYS": app_keys, "AUTH_SECRET": auth_secret,
        "OIDC_CLIENT_ID": "nextrs-test-client", "OIDC_CLIENT_SECRET": client_secret, "NEXTRS_VARIANT": "as-needed",
    }
    flags = ["--net-allow", upstream, "--net-allow", issuer, "--net-allow-addr", "127.0.0.1"]
    for name in ("NEXTRS_UPSTREAM", "SESSION_SECRET", "OIDC_ISSUER", "APP_KEYS", "AUTH_SECRET", "OIDC_CLIENT_ID", "OIDC_CLIENT_SECRET", "NEXTRS_UNLISTED_PORT"):
        flags += ["--js-env", name]
    body_file = str(ROOT / "target/bench-action-body.txt")

    def mint(kid):
        req = urllib.request.Request(f"{issuer}/mint", data=json.dumps({"kid": kid, "claims": {"sub": "u-bench", "aud": "nextrs-api", "exp": int(time.time()) + 7200}}).encode(), method="POST")
        return json.loads(urllib.request.urlopen(req).read())["token"]

    # The application's ES256 session, signed with node:crypto.
    es256_session = node(
        """import { createPrivateKey, sign } from "node:crypto";
const keys = JSON.parse(process.env.APP_KEYS); const b = (v) => Buffer.from(v).toString("base64url"); const now = Math.floor(Date.now() / 1000);
const input = `${b(JSON.stringify({ alg: "ES256" }))}.${b(JSON.stringify({ userId: "u-bench", name: "Bench", iss: "nextrs-jwks-app", aud: "nextrs-jwks-app", iat: now, exp: now + 7200 }))}`;
console.log(`${input}.${b(sign("sha256", Buffer.from(input), { key: createPrivateKey(keys.ES256.private), dsaEncoding: "ieee-p1363" }))}`);""",
        env={**os.environ, "APP_KEYS": app_keys},
    )
    # An Auth.js session, minted with the fixture's own @auth/core/jwt.
    authjs_dir = ROOT / "fixtures/next-authjs"
    authjs_session = node(
        """import { createRequire } from "node:module"; import { pathToFileURL } from "node:url";
const require = createRequire(process.argv[1] + "/package.json"); const jwt = await import(pathToFileURL(require.resolve("@auth/core/jwt")).href);
console.log(await jwt.encode({ token: { sub: "u-bench", name: "Bench", email: "bench@example.test" }, secret: process.env.AUTH_SECRET, salt: "authjs.session-token", maxAge: 7200 }));""",
        str(authjs_dir), env={**os.environ, "AUTH_SECRET": auth_secret},
    )

    jwks_dir = ROOT / "fixtures/next-jwks-proxy"
    jwks_manifest = json.loads((jwks_dir / ".next/server/server-reference-manifest.json").read_text())
    record = next(k for k, v in jwks_manifest["node"].items() if v["exportedName"] == "recordProtected")
    content_type = action_body(body_file)
    hs256 = f"Cookie: session={session_token(secret, 'u-alice', 'Alice')}"
    es256 = f"Cookie: session={es256_session}"
    intl_dir = ROOT / "fixtures/next-intl-proxy"

    fixtures = [
        ("next-auth-proxy", ROOT / "fixtures/next-auth-proxy", ROOT / "fixtures/next-auth-proxy", [
            ("HS256 session (jose)", "/account", [hs256], False),
        ]),
        ("next-jwks-proxy", jwks_dir, jwks_dir, [
            ("RS256 bearer via JWKS", "/api/me", [f"Authorization: Bearer {mint('rs-1')}"], False),
            ("ES256 bearer via JWKS", "/api/me", [f"Authorization: Bearer {mint('es-1')}"], False),
            ("EdDSA bearer via JWKS", "/api/me", [f"Authorization: Bearer {mint('ed-1')}"], False),
            ("ES256 session page", "/account", [es256], False),
            ("protected ISR", "/reports", [es256], False),
            ("protected Server Action", "/actions/protected", [es256, f"Next-Action: {record}", "Accept: text/x-component", "Origin: ORIGIN"], True),
            ("external rewrite (proxy)", "/ext/bench", [], False),
        ]),
        ("next-authjs", authjs_dir, authjs_dir, [
            ("Auth.js session, route handler", "/api/private", [f"Cookie: authjs.session-token={authjs_session}"], False),
            ("Auth.js session, page", "/dashboard", [f"Cookie: authjs.session-token={authjs_session}"], False),
        ]),
        ("next-intl-proxy (as-needed)", intl_dir, intl_dir / ".next-as-needed", [
            ("locale proxy, unprefixed", "/about", [], False),
            ("locale proxy, prefixed", "/de/about", [], False),
        ]),
        ("next-platform-proxy", ROOT / "fixtures/next-platform-proxy", ROOT / "fixtures/next-platform-proxy", [
            ("proxy with outbound fetch", "/gate/open", [], False),
        ]),
    ]
    if args.only_fixture:
        fixtures = [f for f in fixtures if f[0] in args.only_fixture]

    results = {"environment": phase2b_bench.environment(), "config": vars(args), "load": []}
    try:
        for fixture, project, serve, paths in fixtures:
            servers = {"next-start": (["npx", "next", "start", "-H", "0.0.0.0", "-p", "9440"], 9440, None)}
            if args.nextrs_2b:
                servers["2b"] = ([str(Path(args.nextrs_2b).resolve()), "start", str(serve), "--port", "9441", "--js-runtime", "embedded-v8", *flags, "--metrics-addr", "127.0.0.1:9451"], 9441, 9451)
            nextrs = str(ROOT / "target/release/nextrs")
            servers["2c"] = ([nextrs, "start", str(serve), "--port", "9442", "--js-runtime", "embedded-v8", *flags, "--metrics-addr", "127.0.0.1:9452"], 9442, 9452)
            servers["2c-i4"] = ([nextrs, "start", str(serve), "--port", "9443", "--js-runtime", "embedded-v8", "--isolates", "4", *flags, "--metrics-addr", "127.0.0.1:9453"], 9443, 9453)
            results["load"] += phase2b_bench.measure(fixture, project, servers, paths, env, args, body_file, content_type)
    finally:
        for proc in procs:
            os.killpg(proc.pid, 15)
    out = ROOT / args.out
    out.mkdir(parents=True, exist_ok=True)
    target = out / f"phase2c-{time.strftime('%Y%m%dT%H%M%S')}.json"
    target.write_text(json.dumps(results, indent=2))
    print(f"wrote {target}")


if __name__ == "__main__":
    main()
