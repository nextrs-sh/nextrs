// HTTP checks of a NextRS server started from an npm-installed package,
// against the fixtures/next-app build. Used by scripts/npm-install-suite.sh
// in environments that have Node.js (npm needs it) but may have no curl.
// Node.js only sends the requests; it does not serve them.
//
//   node tests/npm/probe.mjs --url http://127.0.0.1:P --project DIR [--only ssr]
//
// Prints one JSON object per check: {"check","result":"pass"|"fail","detail"}.
// The Server Action id and a static chunk are read from DIR/.next.
import fs from "node:fs";
import path from "node:path";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((acc, a, i, all) => (a.startsWith("--") ? [...acc, [a.slice(2), all[i + 1]]] : acc), []),
);
const url = args.url;
const project = args.project;
const only = args.only ? new Set(args.only.split(",")) : null;
let failed = false;
const report = (check, ok, detail) => {
  if (!ok) failed = true;
  console.log(JSON.stringify({ check, result: ok ? "pass" : "fail", detail: String(detail).slice(0, 400) }));
};
const get = async (p, init = {}) => {
  const r = await fetch(url + p, { redirect: "follow", signal: AbortSignal.timeout(30_000), ...init });
  return { status: r.status, headers: r.headers, body: Buffer.from(await r.arrayBuffer()) };
};
const want = (name) => !only || only.has(name);

function action(name, file) {
  const m = JSON.parse(fs.readFileSync(path.join(project, ".next/server/server-reference-manifest.json"), "utf8"));
  const hit = Object.entries(m.node).find(([, v]) => v.exportedName === name && v.filename === file);
  if (!hit) throw new Error(`no Server Action ${name} in ${file}`);
  const page = Object.keys(hit[1].workers)[0].replace(/^app/, "").replace(/\/page$/, "") || "/";
  return { id: hit[0], page };
}

async function run(name, fn) {
  if (!want(name)) return;
  try {
    await fn();
  } catch (e) {
    report(name, false, `${e.name}: ${e.message}`);
  }
}

await run("ssr", async () => {
  const r = await get("/books/42");
  const html = r.body.toString();
  report("ssr", r.status === 200 && /text\/html/.test(r.headers.get("content-type")) && html.includes("42"),
    `GET /books/42 -> ${r.status} ${r.headers.get("content-type")}, ${r.body.length} bytes`);
});

await run("prerendered", async () => {
  const r = await get("/");
  report("prerendered", r.status === 200, `GET / -> ${r.status}`);
});

await run("rsc", async () => {
  const r = await get("/books/42", { headers: { RSC: "1" } });
  const ct = r.headers.get("content-type") ?? "";
  report("rsc", r.status === 200 && ct.startsWith("text/x-component") && /^0:/m.test(r.body.toString()),
    `GET /books/42 RSC: 1 -> ${r.status} ${ct}, ${r.body.length} bytes`);
});

await run("server-action", async () => {
  const a = action("whoAmI", "app/actions/server.ts");
  const call = (user, tag, delay) =>
    get(a.page, {
      method: "POST",
      headers: {
        "Next-Action": a.id,
        Accept: "text/x-component",
        "Content-Type": "text/plain;charset=UTF-8",
        Origin: url,
        "x-test-user": user,
        Cookie: "nextrs-flavor=mint",
      },
      body: JSON.stringify([tag, delay]),
    });
  const r = await call("alice", "probe-tag", 0);
  const body = r.body.toString();
  report("server-action", r.status === 200 && body.includes('"tag":"probe-tag"') && body.includes('"user":"alice"') && body.includes('"cookie":"mint"'),
    `POST ${a.page} whoAmI -> ${r.status} ${(body.match(/"tag":[^}]*/) ?? [""])[0]}`);
  // Two identities at once, each waiting inside the action: neither may see the other's headers.
  const [x, y] = await Promise.all([call("alice", "iso-alice", 300), call("bob", "iso-bob", 300)]);
  const ok = x.body.toString().includes('"tag":"iso-alice","user":"alice","userAfterDelay":"alice"') &&
    y.body.toString().includes('"tag":"iso-bob","user":"bob","userAfterDelay":"bob"');
  report("concurrent-identities", ok, `alice ${x.status}, bob ${y.status}`);
});

await run("static", async () => {
  const dir = path.join(project, ".next/static/chunks");
  const chunk = fs.readdirSync(dir).find((f) => f.endsWith(".js"));
  const bytes = fs.statSync(path.join(dir, chunk)).size;
  const r = await get(`/_next/static/chunks/${chunk}`);
  const cc = r.headers.get("cache-control") ?? "";
  report("static", r.status === 200 && r.body.length === bytes && cc.includes("immutable"),
    `GET /_next/static/chunks/${chunk} -> ${r.status}, ${r.body.length}/${bytes} bytes, cache-control: ${cc}`);
});

process.exit(failed ? 1 : 0);
