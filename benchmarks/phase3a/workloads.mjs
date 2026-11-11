// Phase 3A workload matrix. Each group is one fixture build served by one
// server process; each workload is a request spec for the load generator
// (benchmarks/loadgen) with its correctness oracle: expected status,
// required and forbidden body substrings on EVERY response, and where a
// workload mutates, a ledger check that each acknowledged mutation ran
// exactly once.
//
// Requests and oracles come from the comparison suites (tests/compat) and
// earlier drivers; see docs/research/phase-3a-baseline.md for the mapping.
// Fixtures must be built by benchmarks/phase3a/build-fixtures.sh (ledger
// upstream on LEDGER_PORT during the build, image remote port 4317).
import { readFileSync, rmSync } from "node:fs";
import { createHmac } from "node:crypto";
import { join } from "node:path";
import { spawn } from "node:child_process";
import { request, sleep } from "./lib.mjs";

export const LEDGER_PORT = 4320;
export const IMAGE_PORT = 4317;
const LEDGER = `http://127.0.0.1:${LEDGER_PORT}`;
const SESSION_SECRET = "phase3a-benchmark-session-secret-0123456789";

// ---------------------------------------------------------------- helpers

const html = { accept: "text/html" };

function actionId(ctx, fixture, exportedName, filename) {
  const m = JSON.parse(readFileSync(join(ctx.root, fixture, ".next/server/server-reference-manifest.json"), "utf8"));
  const found = Object.entries(m.node).find(([, v]) => v.exportedName === exportedName && v.filename === filename);
  if (!found) throw new Error(`no action ${exportedName} in ${filename}`);
  return found[0];
}

const buildId = (ctx, fixture) => readFileSync(join(ctx.root, fixture, ".next/BUILD_ID"), "utf8").trim();

function staticChunk(ctx, fixture) {
  const m = JSON.parse(readFileSync(join(ctx.root, fixture, ".next/build-manifest.json"), "utf8"));
  const file = m.rootMainFiles?.[0] ?? m.pages?.["/_app"]?.[0];
  const content = readFileSync(join(ctx.root, fixture, ".next", file));
  // Head and tail of the file: the whole asset arrived, not a prefix.
  return { path: `/_next/${file}`, expect: [content.subarray(0, 48).toString("latin1"), content.subarray(-48).toString("latin1")] };
}

function multipart(fields, boundary = "----nextrsbench7c1f") {
  let s = "";
  for (const [name, value] of fields) s += `--${boundary}\r\ncontent-disposition: form-data; name="${name}"\r\n\r\n${value}\r\n`;
  return { body: s + `--${boundary}--\r\n`, type: `multipart/form-data; boundary=${boundary}` };
}

function jwt(payload) {
  const b64 = (o) => Buffer.from(JSON.stringify(o)).toString("base64url");
  const head = `${b64({ alg: "HS256" })}.${b64(payload)}`;
  return `${head}.${createHmac("sha256", SESSION_SECRET).update(head).digest("base64url")}`;
}

/** Ledger ids recorded under `key` that belong to load-generator run `run`. */
async function ledgerIds(key, run) {
  const r = await fetch(`${LEDGER}/mutations?key=${encodeURIComponent(key)}`);
  const { ids } = await r.json();
  return ids.filter((id) => id.includes(run));
}

/**
 * Exactly-once check over the measured load-generator invocation (its own
 * run id): every mutation request acknowledged with a passing response
 * (`passed_any_by_index`, which counts every response, not only the
 * measured window) must be in the ledger exactly once, and no request of
 * the invocation may have failed (an unacknowledged mutation's outcome is
 * unknown).
 */
function exactlyOnce(key, mutationIndexes) {
  return async (ctx, res) => {
    await sleep(300);
    const ids = await ledgerIds(key, res.run_id);
    const expected = mutationIndexes.reduce((a, i) => a + (res.passed_any_by_index[i] ?? 0), 0);
    const distinct = new Set(ids).size;
    return { ok: res.failed_any === 0 && ids.length === expected && distinct === ids.length, expected, recorded: ids.length, duplicates: ids.length - distinct, failed_any: res.failed_any };
  };
}

async function hits(key) {
  try {
    return Number((await (await fetch(`${LEDGER}/hits?key=${key}`)).text()).trim()) || 0;
  } catch {
    return null;
  }
}

// ---------------------------------------------------------------- services

function startNode(ctx, args, readyUrl) {
  const child = spawn(ctx.node, args, { stdio: "ignore", detached: true, cwd: ctx.root });
  return {
    child,
    async ready() {
      for (let i = 0; i < 100; i++) {
        try { if ((await fetch(readyUrl)).status) return; } catch {}
        await sleep(100);
      }
      throw new Error(`service ${args[0]} did not start`);
    },
    stop: async () => { try { process.kill(-child.pid, "SIGTERM"); } catch {} await sleep(200); },
  };
}

/** The mutation/hit ledger (tests/compat/upstream.mjs). */
async function ledger(ctx) {
  const s = startNode(ctx, [join(ctx.root, "tests/compat/upstream.mjs"), "--port", String(LEDGER_PORT)], `${LEDGER}/hits?key=ping`);
  await s.ready();
  return s;
}

/** Ledger plus the deterministic remote image upstream on IMAGE_PORT. */
async function ledgerAndImages(ctx, fixture) {
  const l = await ledger(ctx);
  const script = `import { startUpstream } from ${JSON.stringify(join(ctx.root, "tests/compat/image-upstream.mjs"))}; startUpstream(${JSON.stringify(join(ctx.root, fixture, "public/photos"))}, ${IMAGE_PORT}, "127.0.0.1");`;
  const img = startNode(ctx, ["--input-type=module", "-e", script], `http://127.0.0.1:${IMAGE_PORT}/img/photo.jpg`);
  await img.ready();
  return { stop: async () => { await img.stop(); await l.stop(); } };
}

const upstreamEnv = () => ({ NEXTRS_UPSTREAM: LEDGER, NEXTRS_IMAGE_UPSTREAM_PORT: String(IMAGE_PORT) });
const upstreamArgs = () => ["--js-env", "NEXTRS_UPSTREAM", "--net-allow", LEDGER, "--net-allow", `http://127.0.0.1:${IMAGE_PORT}`, "--net-allow-addr", "127.0.0.1"];

// ---------------------------------------------------------------- groups

export const groups = [
  {
    id: "app",
    fixture: "fixtures/next-app",
    services: ledger,
    env: upstreamEnv,
    nextrsArgs: upstreamArgs,
    workloads: [
      {
        id: "static-file",
        label: "Static file (/_next/static chunk)",
        spec: (ctx) => {
          const c = staticChunk(ctx, "fixtures/next-app");
          return { requests: [{ path: c.path, expect_body: c.expect }] };
        },
      },
      {
        id: "prerendered",
        label: "Prerendered App Router page",
        spec: { requests: [{ path: "/", headers: [["accept", "text/html"]], expect_body: ['id="title">NextRS static fixture', "should be prerendered at build time"] }] },
      },
      {
        id: "ssr",
        label: "Dynamic App Router SSR (/books/[id])",
        spec: { requests: [{ path: "/books/42", headers: [["accept", "text/html"]], expect_body: ["Book <!-- -->42", "id=<!-- -->42", "</html>"] }] },
      },
      {
        id: "ssr-identity",
        label: "Dynamic SSR reading headers and cookies",
        spec: { requests: [{ path: "/request-info", templated: true, headers: [["accept", "text/html"], ["x-nextrs-probe", "p{run}-{seq}"], ["cookie", "theme=t{seq}"]], expect_body: ["x-nextrs-probe=<!-- -->p{run}-{seq}", "</html>"] }] },
      },
      {
        id: "rsc",
        label: "React Server Components request",
        spec: { requests: [{ path: "/books/42?_rsc", headers: [["rsc", "1"]], expect_body: ['"42"'], forbid_body: ["<html"] }] },
      },
      {
        id: "streaming",
        label: "Streaming SSR (Suspense, 50 ms boundary)",
        spec: { requests: [{ path: "/dynamic", headers: [["accept", "text/html"]], expect_body: ["Noli Me Tangere", "Streamed after Suspense boundary"] }] },
      },
      {
        id: "action",
        label: "Server Action (exactly-once mutation)",
        spec: (ctx) => ({
          requests: [{
            method: "POST", path: "/actions/mutation", templated: true,
            headers: [["next-action", actionId(ctx, "fixtures/next-app", "recordMutation", "app/actions/server.ts")], ["accept", "text/x-component"], ["content-type", "text/plain;charset=UTF-8"], ["origin", `http://127.0.0.1:${ctx.port}`]],
            body: '["bench","m-{run}-{seq}"]',
            expect_body: ['"recorded":true', "m-{run}-{seq}"],
          }],
        }),
        postcheck: exactlyOnce("bench", [0]),
      },
      {
        id: "action-multipart",
        label: "Multipart Server Action (FormData)",
        spec: (ctx) => {
          const id = actionId(ctx, "fixtures/next-app", "echoForm", "app/actions/server.ts");
          const m = multipart([["_1_$ACTION_REF_1", ""], ["_1_$ACTION_1:0", JSON.stringify({ id, bound: "$@1" })], ["_1_$ACTION_1:1", "[null]"], ["_1_title", "Dune"], ["_1_author", "Frank Herbert"], ["_1_note", "n{run}-{seq}"], ["_1_empty", ""], ["0", '[null,"$K1"]']]);
          return { requests: [{ method: "POST", path: "/actions/form", templated: true, headers: [["next-action", id], ["accept", "text/x-component"], ["content-type", m.type], ["origin", `http://127.0.0.1:${ctx.port}`]], body: m.body, expect_body: ["Dune", "Frank Herbert", "n{run}-{seq}"] }] };
        },
      },
      {
        id: "isr-hit",
        label: "ISR page, cache hit",
        spec: { requests: [{ path: "/isr/two", headers: [["accept", "text/html"]], expect_body: ['id="title">slug two', "stamp-"] }] },
      },
      {
        id: "isr-regenerating",
        label: "ISR page with revalidate 2 s (stale serving + background regeneration)",
        spec: { requests: [{ path: "/isr/timed", headers: [["accept", "text/html"]], expect_body: ["stamp-"] }] },
        beforeMeasure: async (ctx) => { ctx.isrHits0 = await hits("isr-timed"); },
        postcheck: async (ctx) => {
          const h = await hits("isr-timed");
          // Information, not a pass/fail gate: regenerations during the window.
          return { ok: true, regenerations: h == null || ctx.isrHits0 == null ? null : h - ctx.isrHits0 };
        },
      },
    ],
  },
  {
    id: "cache-components",
    fixture: "fixtures/next-cache-components",
    services: ledger,
    env: upstreamEnv,
    nextrsArgs: upstreamArgs,
    readyPath: "/api/health",
    workloads: [
      {
        id: "use-cache-hit",
        label: 'Cache Components ("use cache" hit, dynamic shell)',
        spec: { requests: [{ path: "/cached?shelf=bench", headers: [["accept", "text/html"]], expect_body: ["shelf=bench hit=1 at="] }] },
      },
    ],
  },
  {
    id: "proxy",
    fixture: "fixtures/next-proxy",
    services: ledger,
    env: (ctx) => ({ ...upstreamEnv(), NEXTRS_CLOSED_PORT: "1" }),
    nextrsArgs: () => [...upstreamArgs(), "--js-env", "NEXTRS_CLOSED_PORT"],
    workloads: [
      {
        id: "proxy-matched-ssr",
        label: "Proxy-matched dynamic page",
        spec: { requests: [{ path: "/dynamic", headers: [["accept", "text/html"]], expect_body: ["set-by-proxy", "</html>"] }] },
      },
      {
        id: "proxy-matched-static",
        label: "Proxy-matched prerendered page",
        spec: { requests: [{ path: "/target", headers: [["accept", "text/html"]], expect_body: ['id="title">target'] }] },
      },
      {
        id: "proxy-unmatched",
        label: "Proxy-unmatched prerendered page",
        spec: { requests: [{ path: "/excluded", headers: [["accept", "text/html"]], expect_body: ['id="title">excluded'] }] },
      },
    ],
  },
  {
    id: "auth",
    fixture: "fixtures/next-auth-proxy",
    env: () => ({ SESSION_SECRET }),
    nextrsArgs: () => ["--js-env", "SESSION_SECRET"],
    readyPath: "/api/health",
    workloads: [
      {
        id: "jwt-valid",
        label: "JWT session (jose, HS256) verified in the proxy and the page",
        spec: () => {
          const now = Math.floor(Date.now() / 1000);
          const token = jwt({ userId: "bench-user", name: "bench", iat: now, exp: now + 86400 });
          return { requests: [{ path: "/dashboard", headers: [["accept", "text/html"], ["cookie", `session=${token}`]], expect_body: ["dashboard of <!-- -->bench", "bench-user"] }] };
        },
      },
      {
        id: "jwt-rejected",
        label: "JWT proxy redirecting an anonymous request",
        spec: { requests: [{ path: "/dashboard", headers: [["accept", "text/html"]], expect_status: 307, forbid_body: ["dashboard of"] }] },
      },
    ],
  },
  {
    id: "pages",
    fixture: "fixtures/next-pages",
    services: ledger,
    env: upstreamEnv,
    nextrsArgs: upstreamArgs,
    workloads: [
      {
        id: "pages-ssr",
        label: "Pages Router getServerSideProps (no proxy match)",
        spec: { requests: [{ path: "/ssr/whoami", templated: true, headers: [["accept", "text/html"], ["cookie", "user=u{run}{seq}"]], expect_body: ["secret-of-u{run}{seq}", "__NEXT_DATA__"] }] },
      },
      {
        id: "pages-ssr-proxied",
        label: "Pages Router getServerSideProps behind the proxy",
        spec: { requests: [{ path: "/books/42?ref=b{seq}", templated: true, headers: [["accept", "text/html"]], expect_body: ["Book <!-- -->42", '"gssp":true', "b{seq}"] }] },
      },
      {
        id: "pages-api-get",
        label: "Pages Router API route (GET)",
        spec: { requests: [{ path: "/api/health", expect_body: ['"status":"ok"'] }] },
      },
      {
        id: "pages-api-post",
        label: "Pages Router API route (POST JSON, proxied)",
        spec: { requests: [{ method: "POST", path: "/api/echo", templated: true, headers: [["content-type", "application/json"]], body: '{"bench":"{run}-{seq}","items":[0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31]}', expect_body: ['"bench":"{run}-{seq}"', "31]"] }] },
      },
      {
        id: "pages-data",
        label: "Pages Router /_next/data (getServerSideProps)",
        spec: (ctx) => ({ requests: [{ path: `/_next/data/${buildId(ctx, "fixtures/next-pages")}/books/42.json?x=1`, expect_body: ['"__N_SSP":true', '"id":"42"'] }] }),
      },
    ],
  },
  {
    id: "image",
    fixture: "fixtures/next-image",
    services: async (ctx) => {
      // next start keeps its image cache on disk: start from empty, as NextRS (memory cache) does.
      rmSync(join(ctx.root, "fixtures/next-image/.next/cache/images"), { recursive: true, force: true });
      return ledgerAndImages(ctx, "fixtures/next-image");
    },
    env: upstreamEnv,
    nextrsArgs: upstreamArgs,
    workloads: [
      {
        id: "image-hit",
        label: "Image optimizer, cache hit (640 px WebP)",
        spec: { requests: [{ path: "/_next/image?url=%2Fphotos%2Fphoto.jpg&w=640&q=75", headers: [["accept", "image/webp"]], expect_body: ["RIFF", "WEBP"] }] },
      },
      {
        id: "image-transform",
        label: "Image optimizer, transformation on every request (remote JPEG to 640 px WebP)",
        connections: 8,
        spec: { requests: [{ path: `/_next/image?url=${encodeURIComponent(`http://127.0.0.1:${IMAGE_PORT}/img/photo.jpg?bench=`)}{run}-{seq}&w=640&q=75`, templated: true, headers: [["accept", "image/webp"]], expect_body: ["RIFF", "WEBP"] }] },
      },
    ],
  },
  {
    id: "mixed",
    fixture: "fixtures/next-soak-mixed",
    services: (ctx) => ledgerAndImages(ctx, "fixtures/next-soak-mixed"),
    env: upstreamEnv,
    nextrsArgs: upstreamArgs,
    workloads: [
      {
        id: "mixed",
        label: "Mixed App/Pages Router, API, actions, ISR, images (Phase 2G soak weights)",
        spec: (ctx) => mixedSpec(ctx),
        postcheck: (ctx, res) => exactlyOnce("soak", ctx.mixedMutationIndexes)(ctx, res),
      },
    ],
  },
];

/**
 * The Phase 2G soak mix (benchmarks/phase2g_soak.mjs) without client
 * aborts, restart injection or random image misses (a closed-loop
 * benchmark must offer the same sequence to every server): requests in a
 * fixed interleaved order, weights as in the soak.
 */
function mixedSpec(ctx) {
  const F = "fixtures/next-soak-mixed";
  const echoId = actionId(ctx, F, "soakEcho", "app/soak/action/actions.ts");
  const bid = buildId(ctx, F);
  const origin = `http://127.0.0.1:${ctx.port}`;
  const img = (url, w) => `/_next/image?url=${encodeURIComponent(url)}&w=${w}&q=75`;
  const kinds = [
    ...["photo.jpg", "odd.jpg", "p3.jpg", "rotated.jpg"].flatMap((s) => [256, 640, 1080].map((w) => ({ name: "img-hit", w: 22 / 12, r: { path: img(`/photos/${s}`, w), headers: [["accept", "image/webp,*/*"]], expect_body: ["WEBP"] } }))),
    { name: "img-remote-miss", w: 6, r: { path: img(`http://127.0.0.1:${IMAGE_PORT}/img/photo.jpg?n={seq}`, 256).replace("%7Bseq%7D", "{seq}"), templated: true, headers: [["accept", "image/webp"]], expect_body: ["WEBP"] } },
    { name: "app-ssr", w: 8, r: { path: "/soak/ssr", templated: true, headers: [["x-soak-token", "t{run}-{seq}"], ["accept", "text/html"]], expect_body: ["t{run}-{seq}", "</html>"] } },
    { name: "app-action", w: 4, r: { method: "POST", path: "/soak/action", templated: true, headers: [["next-action", echoId], ["accept", "text/x-component"], ["content-type", "text/plain;charset=UTF-8"], ["origin", origin]], body: '["a{run}-{seq}"]', expect_body: ["echo:a{run}-{seq}"] } },
    { name: "app-isr", w: 3, r: { path: "/soak/isr", headers: [["accept", "text/html"]], expect_body: ["</html>"] } },
    { name: "app-proxy", w: 3, r: { path: "/soak/proxied/s{seq}", templated: true, headers: [["x-soak-token", "x{run}"], ["accept", "text/html"]], expect_body: ["s{seq}:x{run}"] } },
    { name: "pages-ssr", w: 7, r: { path: "/p/ssr/7", templated: true, headers: [["x-soak-token", "p{run}-{seq}"]], expect_body: ["7:p{run}-{seq}"] } },
    { name: "pages-data", w: 4, r: { path: `/_next/data/${bid}/p/ssr/9.json`, templated: true, headers: [["x-soak-token", "d{run}-{seq}"]], expect_body: ['"pageProps"', "d{run}-{seq}"] } },
    { name: "pages-ssg", w: 4, r: { path: "/p/ssg", expect_body: ['"when":"build"'] } },
    { name: "pages-isr", w: 3, r: { path: "/p/isr", expect_body: ["</html>"] } },
    { name: "pages-api-get", w: 4, r: { path: "/api/p/echo", templated: true, headers: [["x-soak-token", "e{run}-{seq}"]], expect_body: ["e{run}-{seq}"] } },
    { name: "pages-api-post", w: 3, mutation: true, r: { method: "POST", path: "/api/p/mutate?id=m{run}-{seq}", templated: true, expect_body: ["m{run}-{seq}"] } },
    { name: "pages-preview", w: 2, r: { path: "/p/preview", expect_body: ["false:-"] } },
    { name: "pages-secure-anon", w: 2, r: { path: "/p/secure", expect_status: 307, forbid_body: ["soak-secret"] } },
    { name: "pages-secure-session", w: 1, r: { path: "/p/secure", headers: [["cookie", "session=synthetic-valid-session"]], expect_body: ["soak-secret"] } },
  ];
  // Smooth weighted round-robin: a fixed, well-interleaved sequence.
  const total = kinds.reduce((a, k) => a + k.w, 0);
  const current = kinds.map(() => 0);
  const seq = [];
  const n = Math.round(total) * 4;
  for (let i = 0; i < n; i++) {
    kinds.forEach((k, j) => (current[j] += k.w));
    let best = 0;
    for (let j = 1; j < kinds.length; j++) if (current[j] > current[best]) best = j;
    current[best] -= total;
    seq.push(best);
  }
  ctx.mixedLength = seq.length;
  ctx.mixedMutationIndexes = seq.map((k, i) => (kinds[k].mutation ? i : -1)).filter((i) => i >= 0);
  return { requests: seq.map((k) => ({ ...kinds[k].r, kind: kinds[k].name })) };
}
