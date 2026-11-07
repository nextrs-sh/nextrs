// Server Action protocol, security and mutation checks over HTTP, against
// next start (baseline) and nextrs (candidate) serving the same build.
//
//   node tests/compat/actions.mjs --baseline URL --candidate URL \
//     --upstream URL --project fixtures/next-app [--json FILE]
//     [--stall-ms N] [--request-timeout-ms N]
//
// Comparison cases send the same request to both servers and compare
// status, content type, Server Action headers and the (normalized) body,
// and, for mutations, the side effects recorded by the upstream's ledger.
// Candidate-only cases cover NextRS's own limits and failure handling
// (isolate restarts, request deadlines), which next start does not have.
//
// Action IDs are read from the build's server-reference-manifest.json by
// export name; nothing here hardcodes one.
import { createCipheriv, randomBytes } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import http from "node:http";
import { join } from "node:path";
import { corrupt } from "./routing-lib.mjs";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => {
    if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]]);
    return pairs;
  }, []),
);
const { baseline, candidate, upstream, project } = args;
if (!baseline || !candidate || !upstream || !project) {
  console.error("usage: actions.mjs --baseline URL --candidate URL --upstream URL --project DIR [--json FILE]");
  process.exit(2);
}
const stallMs = args["stall-ms"] ? Number(args["stall-ms"]) : null;
const requestTimeoutMs = args["request-timeout-ms"] ? Number(args["request-timeout-ms"]) : null;
const only = args.only ? new Set(args.only.split(",")) : null;
const manifest = JSON.parse(readFileSync(join(project, ".next/server/server-reference-manifest.json"), "utf8"));
const actionId = (exportedName, filename = "app/actions/server.ts") => {
  const found = Object.entries(manifest.node).find(([, v]) => v.exportedName === exportedName && v.filename === filename);
  if (!found) throw new Error(`no action ${exportedName} in ${filename}`);
  return found[0];
};
const ids = {
  record: actionId("recordMutation"),
  who: actionId("whoAmI"),
  echo: actionId("echoForm"),
  flavor: actionId("setFlavor"),
  hang: actionId("neverResolves"),
  large: actionId("largeResult"),
  throws: actionId("throwFromClient"),
};
const run = `${Date.now().toString(36)}-${randomBytes(3).toString("hex")}`;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function ledger(key) {
  return (await (await fetch(`${upstream}/mutations?key=${encodeURIComponent(key)}`)).json()).ids;
}

// A fetch-style action call (what React's callServer sends for arguments
// that encode to a string): text/plain JSON array, Next-Action header.
async function call(base, page, id, actionArgs, { origin = base, headers = {}, method = "POST", body, signal, contentType } = {}) {
  const init = {
    method,
    headers: {
      accept: "text/x-component",
      "next-action": id,
      "content-type": contentType ?? "text/plain;charset=UTF-8",
      ...(origin ? { origin } : {}),
      ...headers,
    },
    signal,
  };
  if (method !== "GET" && method !== "HEAD") init.body = body ?? JSON.stringify(actionArgs);
  const response = await fetch(`${base}${page}`, init);
  return summarize(response);
}

async function summarize(response) {
  const text = await response.text();
  return {
    status: response.status,
    type: (response.headers.get("content-type") ?? "").split(";")[0],
    notFound: response.headers.get("x-nextjs-action-not-found"),
    redirect: response.headers.get("x-action-redirect"),
    cacheControl: response.headers.get("cache-control"),
    setCookie: response.headers.get("set-cookie"),
    // NextRS's own marker; not part of the comparison.
    ...(nextrsMarker(response) ? { nextrsError: nextrsMarker(response) } : {}),
    body: normalize(text),
  };
}

let comparing = false;
const nextrsMarker = (response) => (comparing ? null : response.headers.get("x-nextrs-error"));

// One request on its own connection (no keep-alive reuse): a server may
// answer an oversized body before reading it and close the socket.
function rawCall(base, page, id, body) {
  return new Promise((resolve) => {
    const url = new URL(page, base);
    const request = http.request(
      url,
      {
        method: "POST",
        agent: false,
        headers: { accept: "text/x-component", "next-action": id, "content-type": "text/plain;charset=UTF-8", origin: base, "content-length": Buffer.byteLength(body) },
      },
      (response) => {
        const chunks = [];
        response.on("data", (c) => chunks.push(c));
        response.on("end", () =>
          resolve({
            status: response.statusCode,
            type: (response.headers["content-type"] ?? "").split(";")[0],
            body: normalize(Buffer.concat(chunks).toString()),
            ...(comparing ? {} : { nextrsError: response.headers["x-nextrs-error"] ?? null }),
          }),
        );
      },
    );
    request.on("error", (error) => resolve({ error: error.code ?? error.message }));
    request.end(body);
  });
}

// Error digests hash the server-side stack, which differs between runtimes;
// the build ID is the same for both servers. Ledger keys carry the server.
function normalize(text) {
  return text
    .replace(/"digest":"[^"]*"/g, '"digest":"<digest>"')
    .replaceAll(`${run}-b-`, `${run}-?-`)
    .replaceAll(`${run}-c-`, `${run}-?-`)
    .slice(0, 2000);
}

// The candidate's isolates still answer.
async function healthy() {
  const r = await call(candidate, "/actions/client", ids.who, ["health"]);
  return r.status === 200 && r.body.includes('"health"');
}

function mpaForm(fields) {
  const form = new FormData();
  for (const [name, value] of fields) form.append(name, value);
  return form;
}

async function mpa(base, page, fields, { origin = base, headers = {} } = {}) {
  const response = await fetch(`${base}${page}`, {
    method: "POST",
    headers: { ...(origin ? { origin } : {}), ...headers },
    body: mpaForm(fields),
    redirect: "manual",
  });
  const summary = await summarize(response);
  summary.body = summary.type === "text/html" ? `<html ${summary.body.length} chars>` : summary.body;
  return summary;
}

const decodeEntities = (s) => s.replace(/&quot;/g, '"').replace(/&#x27;/g, "'").replace(/&lt;/g, "<").replace(/&gt;/g, ">").replace(/&amp;/g, "&");

// The hidden inputs React renders into a form for progressive enhancement
// (action id, or the bound/closure reference and its encrypted arguments).
async function formFields(base, page, formId) {
  const html = await (await fetch(`${base}${page}`)).text();
  const start = html.indexOf(`id="${formId}"`);
  if (start === -1) throw new Error(`no form ${formId} on ${page}`);
  const formStart = html.lastIndexOf("<form", start);
  const formEnd = html.indexOf("</form>", start);
  const fields = [];
  for (const m of html.slice(formStart, formEnd).matchAll(/<input([^>]*)>/g)) {
    const attrs = Object.fromEntries([...m[1].matchAll(/([a-zA-Z-]+)="([^"]*)"/g)].map(([, k, v]) => [k, decodeEntities(v)]));
    if (attrs.name) fields.push([attrs.name, attrs.value ?? attrs.defaultValue ?? ""]);
  }
  return { fields, html };
}

// The encrypted bound-arguments field of a closure form ($ACTION_n:1 holds
// a Flight array whose first element is the encrypted string).
function encryptedField(fields) {
  const index = fields.findIndex(([name, value]) => /^\$ACTION_\d+:1$/.test(name) && value.startsWith('["'));
  if (index === -1) throw new Error("no encrypted closure field");
  return index;
}

function withEncrypted(fields, mutate) {
  const index = encryptedField(fields);
  const copy = fields.map((f) => [...f]);
  const array = JSON.parse(copy[index][1]);
  array[0] = mutate(array[0]);
  copy[index][1] = JSON.stringify(array);
  return copy;
}

const cases = {};
const compareCase = (name, fn) => (cases[name] = { kind: "compare", fn });
const candidateCase = (name, fn) => (cases[name] = { kind: "candidate", fn });
// Compared, but a difference is a documented divergence (reported as
// KNOWN-DIFF, not a failure) until the phase named in `note` closes it.
const knownCase = (name, note, fn) => (cases[name] = { kind: "known", note, fn });

// --- Origin and method checks (CSRF) -------------------------------------
compareCase("invalid-origin-fetch", async (base, tag) => {
  const key = `${run}-${tag}-origin`;
  const r = await call(base, "/actions/mutation", ids.record, [key, "x"], { origin: "http://evil.example" });
  await sleep(200);
  return { ...r, recorded: (await ledger(key)).length };
});
compareCase("invalid-origin-mpa", async (base) => {
  const r = await mpa(base, "/actions/cookies", [[`$ACTION_ID_${ids.flavor}`, ""], ["flavor", "stolen"]], { origin: "http://evil.example" });
  return r;
});
compareCase("origin-null", async (base, tag) => {
  const key = `${run}-${tag}-null`;
  const r = await call(base, "/actions/mutation", ids.record, [key, "x"], { origin: "null" });
  await sleep(200);
  return { ...r, recorded: (await ledger(key)).length };
});
// Next trusts x-forwarded-host for the comparison (a browser cannot set it
// cross-origin); NextRS passes it through unchanged, as next start does.
compareCase("forwarded-host-matching-origin", async (base, tag) => {
  const key = `${run}-${tag}-xfh`;
  const r = await call(base, "/actions/mutation", ids.record, [key, "x"], {
    origin: "http://proxy.example",
    headers: { "x-forwarded-host": "proxy.example" },
  });
  return { status: r.status, recorded: (await ledger(key)).length };
});
compareCase("missing-origin", async (base, tag) => {
  const key = `${run}-${tag}-noorigin`;
  const r = await call(base, "/actions/mutation", ids.record, [key, "x"], { origin: null });
  return { status: r.status, recorded: (await ledger(key)).length };
});
compareCase("get-does-not-run-actions", async (base, tag) => {
  const key = `${run}-${tag}-get`;
  const r = await call(base, "/actions/mutation", ids.record, [key, "x"], { method: "GET" });
  await sleep(200);
  return { status: r.status, type: r.type, recorded: (await ledger(key)).length };
});
compareCase("put-and-delete-refused", async (base, tag) => {
  const key = `${run}-${tag}-put`;
  const statuses = [];
  for (const method of ["PUT", "DELETE", "PATCH"]) statuses.push((await call(base, "/actions/mutation", ids.record, [key, "x"], { method })).status);
  await sleep(200);
  return { statuses, recorded: (await ledger(key)).length };
});

// --- Action references ------------------------------------------------------
compareCase("unknown-action-id", (base) => call(base, "/actions/mutation", "40" + "ab".repeat(20), []));
compareCase("malformed-action-id", (base) => call(base, "/actions/mutation", "../../etc/passwd", []));
compareCase("prototype-action-id", (base) => call(base, "/actions/mutation", "__proto__", []));
compareCase("action-id-from-another-build", (base) => call(base, "/actions/mutation", "7c" + "0f".repeat(20), ["k", "i"]));
// recordMutation is not in /actions/simple's worker: Next forwards the
// request to a page that has it (an internal request to its own server).
compareCase("forwarded-to-another-worker", async (base, tag) => {
  const key = `${run}-${tag}-forward`;
  const r = await call(base, "/actions/simple", ids.record, [key, "fwd"]);
  return { status: r.status, type: r.type, body: r.body, recorded: await ledger(key) };
});
compareCase("unmatched-path", (base) => call(base, "/no/such/page", ids.who, ["x"]));
compareCase("args-limit", (base) => call(base, "/actions/client", ids.who, Array.from({ length: 1001 }, () => 0)));

// --- Encrypted closures ------------------------------------------------------
compareCase("closure-mpa", async (base) => {
  const { fields } = await formFields(base, "/actions/bound", "closure-form");
  const r = await mpa(base, "/actions/bound", fields.map(([n, v]) => [n, n === "name" ? "Lin" : v]));
  return { status: r.status, type: r.type };
});
compareCase("closure-tampered", async (base) => {
  const { fields } = await formFields(base, "/actions/bound", "closure-form");
  const tampered = withEncrypted(fields, (b64) => {
    const bytes = Buffer.from(b64, "base64");
    bytes[bytes.length - 3] ^= 0x40;
    return bytes.toString("base64");
  });
  const r = await mpa(base, "/actions/bound", tampered);
  return { status: r.status, type: r.type, leaked: r.body.includes("closure-secret-1d") };
});
compareCase("closure-foreign-key", async (base) => {
  const { fields } = await formFields(base, "/actions/bound", "closure-form");
  const actionRef = JSON.parse(fields.find(([n]) => /^\$ACTION_\d+:0$/.test(n))[1]).id;
  const forged = withEncrypted(fields, () => {
    // Next's format with a key that is not the build's.
    const iv = randomBytes(16);
    const cipher = createCipheriv("aes-256-gcm", randomBytes(32), iv);
    const ct = Buffer.concat([cipher.update(`${actionRef}["forged"]`), cipher.final(), cipher.getAuthTag()]);
    return Buffer.concat([iv, ct]).toString("base64");
  });
  const r = await mpa(base, "/actions/bound", forged);
  return { status: r.status, type: r.type };
});
compareCase("closure-not-base64", async (base) => {
  const { fields } = await formFields(base, "/actions/bound", "closure-form");
  const r = await mpa(base, "/actions/bound", withEncrypted(fields, () => "%%%not-base64%%%"));
  return { status: r.status, type: r.type };
});

// --- Bodies and limits -------------------------------------------------------
compareCase("body-over-next-limit", async (base) => {
  const r = await rawCall(base, "/actions/client", ids.who, JSON.stringify(["x".repeat(1536 * 1024)]));
  return { status: r.status, type: r.type, body: r.body };
});
compareCase("body-under-next-limit", async (base) => {
  const r = await rawCall(base, "/actions/client", ids.who, JSON.stringify(["y".repeat(900 * 1024)]));
  return { status: r.status, type: r.type, hasTag: r.body?.includes("yyyy") };
});
compareCase("multipart-bad-boundary", (base) =>
  call(base, "/actions/form", ids.echo, null, {
    contentType: "multipart/form-data; boundary=nope",
    body: '--other\r\nContent-Disposition: form-data; name="0"\r\n\r\n["$K1"]\r\n--other--\r\n',
  }),
);
compareCase("multipart-truncated", (base) =>
  call(base, "/actions/form", ids.echo, null, {
    contentType: "multipart/form-data; boundary=B",
    body: '--B\r\nContent-Disposition: form-data; name="0"\r\n\r\n["$undefined","$K1"]\r\n--B\r\nContent-Disposition: form-data; name="1_title"\r\n\r\nDu',
  }),
);
compareCase("mpa-multipart-truncated", async (base) => {
  const response = await fetch(`${base}/actions/form`, {
    method: "POST",
    headers: { origin: base, "content-type": "multipart/form-data; boundary=B" },
    body: `--B\r\nContent-Disposition: form-data; name="$ACTION_ID_${ids.echo}"\r\n\r\n\r\n--B\r\nContent-Disposition: form-data; name="title"\r\n\r\nDu`,
  });
  const r = await summarize(response);
  return { status: r.status, type: r.type };
});
compareCase("urlencoded-post", async (base) => {
  const response = await fetch(`${base}/actions/form`, { method: "POST", headers: { origin: base }, body: new URLSearchParams({ title: "x" }) });
  return { status: response.status, type: (response.headers.get("content-type") ?? "").split(";")[0] };
});
compareCase("large-result", async (base) => {
  const r = await call(base, "/actions/client", ids.large, [3 * 1024 * 1024]);
  return { status: r.status, type: r.type };
});

// --- Malformed Flight payloads (React RSC advisory classes) ----------------
// Reply payloads shaped like the published RSC deserialization and DoS
// proofs of concept: self and prototype references, a promise-like "then",
// and deep nesting. Both servers must reject or answer them the same way,
// and the candidate's isolates must keep serving.
const flight = (fields) => {
  const parts = fields.map(([name, value]) => `--F\r\nContent-Disposition: form-data; name="${name}"\r\n\r\n${value}\r\n`);
  return parts.join("") + "--F--\r\n";
};
const hostile = {
  "proto-then": [["0", '["$1:__proto__:then"]'], ["1", '{"then":"$1:__proto__:then","status":"resolved_model","value":"{}"}']],
  "constructor-chain": [["0", '["$1:constructor:constructor"]'], ["1", "{}"]],
  "self-reference": [["0", '["$0"]']],
  "cyclic-promise": [["0", '["$@1"]'], ["1", '"$@0"']],
  "server-ref-to-source": [["0", '["$h1"]'], ["1", `{"id":"${ids.throws}","bound":null}`]],
  "deep-nesting": [["0", `[${"[".repeat(20000)}${"]".repeat(20000)}]`]],
  // Shallow enough for Node's default stack: the isolate's must not be smaller.
  "nesting-3000": [["0", `[${"[".repeat(3000)}${"]".repeat(3000)}]`]],
};
for (const [name, fields] of Object.entries(hostile)) {
  compareCase(`flight-${name}`, async (base) => {
    const r = await call(base, "/actions/client", ids.who, null, { contentType: "multipart/form-data; boundary=F", body: flight(fields) });
    const sourceLeak = /use server|createHash|throwFromClient|function\s*\(/.test(r.body);
    return { status: r.status, type: r.type, notFound: r.notFound, sourceLeak };
  });
}

// --- Cache invalidation from actions (Phase 1E boundary) ------------------
const invalidateId = actionId("invalidate");
async function invalidate(base, how) {
  // The reply React sends for invalidate(formData): multipart, fields of
  // argument 1 prefixed "_1_".
  const form = new FormData();
  form.append("_1_how", how);
  form.append("_1_tag", "actions-revalidate");
  form.append("0", '["$K1"]');
  const response = await fetch(`${base}/actions/revalidate`, {
    method: "POST",
    body: form,
    headers: { origin: base, "next-action": invalidateId, accept: "text/x-component" },
  });
  await response.text();
  return { status: response.status, revalidated: response.headers.get("x-action-revalidated") };
}
const hitOf = async (base) => Number(/<p id="hit">(\d+)<\/p>/.exec(await (await fetch(`${base}/actions/revalidate`)).text())[1]);
const stampOf = async (base, headers = {}) => /stamp-(\d+)/.exec(await (await fetch(`${base}/actions/static-time`, { headers })).text())[1];
for (const how of ["updateTag", "revalidatePath", "refresh"]) {
  compareCase(`invalidate-${how}`, async (base) => {
    const before = await hitOf(base);
    const r = await invalidate(base, how);
    return { ...r, refetched: (await hitOf(base)) > before };
  });
}
// The stale-while-revalidate profile: the first read after it serves the
// cached value on both servers and refreshes it in the background. Runs
// after revalidatePath, whose expired path tag stays in Next's manifest:
// before Phase 1E NextRS refetched here (a stale entry was reported with a
// fake timestamp that the earlier expiry then matched).
compareCase("invalidate-revalidateTag-max", async (base) => {
  const before = await hitOf(base);
  const r = await invalidate(base, "revalidateTag");
  return { ...r, firstReadRefetched: (await hitOf(base)) > before };
});
candidateCase("revalidateTag-max-refresh-timing", async () => {
  const before = await hitOf(candidate);
  await invalidate(candidate, "revalidateTag");
  const first = await hitOf(candidate);
  await sleep(300);
  return { firstRead: first - before, secondRead: (await hitOf(candidate)) - before };
});
// revalidatePath on a prerendered page: both servers regenerate it once
// and serve the stored result afterwards (tests/compat/isr.mjs covers the
// cache states in detail).
compareCase("invalidate-static-path", async (base) => {
  const before = await stampOf(base);
  const r = await invalidate(base, "revalidateStaticPath");
  await sleep(300);
  return { ...r, htmlRerendered: (await stampOf(base)) !== before, rscRerendered: (await stampOf(base, { rsc: "1" })) !== before };
});

// --- Mutations ---------------------------------------------------------------
compareCase("mutation-once", async (base, tag) => {
  const key = `${run}-${tag}-once`;
  const r = await call(base, "/actions/mutation", ids.record, [key, "only"]);
  await sleep(300);
  return { status: r.status, body: r.body.includes('"recorded":true'), ledger: await ledger(key) };
});
compareCase("concurrent-mutations", async (base, tag) => {
  const key = `${run}-${tag}-concurrent`;
  const n = 64;
  const results = await Promise.all(
    Array.from({ length: n }, (_, i) => call(base, "/actions/mutation", ids.record, [key, `c${i}`, i % 5 ? 0 : 30])),
  );
  const answered = results.filter((r, i) => r.status === 200 && r.body.includes(`"c${i}"`)).length;
  const recorded = await ledger(key);
  return { answered, recorded: recorded.length, distinct: new Set(recorded).size };
});
compareCase("concurrent-identity", async (base) => {
  const n = 48;
  const results = await Promise.all(
    Array.from({ length: n }, (_, i) =>
      call(base, "/actions/client", ids.who, [`t${i}`, (i % 4) * 25], { headers: { "x-test-user": `user-${i}`, cookie: `nextrs-flavor=f${i}` } }),
    ),
  );
  const leaks = results.filter((r, i) => {
    const want = [`"t${i}"`, `"user-${i}"`, `"f${i}"`];
    return r.status !== 200 || !want.every((w) => r.body.includes(w)) || (r.body.match(/"user-\d+"/g) ?? []).some((u) => u !== `"user-${i}"`);
  }).length;
  return { n, leaks };
});
// The client leaves after the action committed, and before it did. Neither
// server cancels a running action: both side effects happen, once.
compareCase("disconnect-after-commit", async (base, tag) => {
  const key = `${run}-${tag}-after`;
  const controller = new AbortController();
  const pending = call(base, "/actions/mutation", ids.record, [key, "a", 1500], { signal: controller.signal }).catch((e) => e.name);
  await sleep(400);
  controller.abort();
  const outcome = await pending;
  await sleep(2000);
  return { outcome, ledger: await ledger(key) };
});
compareCase("disconnect-before-commit", async (base, tag) => {
  const key = `${run}-${tag}-before`;
  const controller = new AbortController();
  const pending = call(base, "/actions/mutation", ids.record, [key, "b", 0, 0, 1200], { signal: controller.signal }).catch((e) => e.name);
  await sleep(300);
  controller.abort();
  const outcome = await pending;
  await sleep(2500);
  return { outcome, ledger: await ledger(key) };
});

// --- Candidate only: NextRS limits and failures ------------------------------
candidateCase("body-over-nextrs-limit", async () => {
  // Above --max-request-body (10 MiB): refused with a 413 before
  // JavaScript runs (the client may see the socket close mid-upload).
  const r = await rawCall(candidate, "/actions/client", ids.who, JSON.stringify(["z".repeat(11 * 1024 * 1024)]));
  return { status: r.status, nextrsError: r.nextrsError, error: r.error, healthy: await healthy() };
});
candidateCase("mpa-too-many-parts", async () => {
  const fields = [[`$ACTION_ID_${ids.echo}`, ""]];
  for (let i = 0; i < 5000; i++) fields.push([`f${i}`, "x"]);
  const r = await mpa(candidate, "/actions/form", fields);
  return { status: r.status, healthy: await healthy() };
});
if (stallMs) {
  // The action commits, then holds the CPU past the stall timeout: the
  // isolate is terminated. The client gets a 502 (the request was
  // delivered, so NextRS does not retry it) and the mutation is not
  // repeated by the restart.
  candidateCase("stall-after-commit", async () => {
    const key = `${run}-stall`;
    const r = await call(candidate, "/actions/mutation", ids.record, [key, "s", 0, stallMs + 2000]);
    await sleep(stallMs + 3000);
    const recovered = await healthy();
    await sleep(1000);
    return { status: r.status, nextrsError: r.nextrsError, ledger: await ledger(key), recovered };
  });
}
if (requestTimeoutMs) {
  candidateCase("never-resolving-action", async () => {
    const started = Date.now();
    const r = await call(candidate, "/actions/mutation", ids.hang, []);
    return { status: r.status, nextrsError: r.nextrsError, withinDeadline: Date.now() - started < requestTimeoutMs + 6000, healthy: await healthy() };
  });
}
candidateCase("closures-across-isolates", async () => {
  // Each render encrypts its closure in whichever isolate served it; the
  // submission may be decrypted by another.
  let ok = 0;
  const n = 24;
  await Promise.all(
    Array.from({ length: n }, async () => {
      const { fields, html } = await formFields(candidate, "/actions/bound-dynamic", "closure-form");
      const rendered = /<p id="rendered">([^<]*)<\/p>/.exec(html)?.[1];
      const response = await fetch(`${candidate}/actions/bound-dynamic`, { method: "POST", headers: { origin: candidate }, body: mpaForm(fields) });
      const text = await response.text();
      if (response.status === 200 && rendered && text.includes(`from ${rendered}`)) ok++;
    }),
  );
  return { n, ok };
});

// ---------------------------------------------------------------------------
// What each critical case must observe, on both servers (Phase 2C audit):
// equal ledgers are not enough (a malformed request body once made both
// servers record nothing). next start contradicting one is ORACLE-FAIL (the
// test is wrong), NextRS contradicting one is FAIL.
const nothingRecorded = (o) => (o.recorded === 0 ? null : `recorded ${o.recorded}, expected 0`);
const ledgerIs = (...ids) => (o) => (JSON.stringify(o.ledger ?? o.recorded) === JSON.stringify(ids) ? null : `ledger ${JSON.stringify(o.ledger ?? o.recorded)}, expected ${JSON.stringify(ids)}`);
const EXPECT = {
  "invalid-origin-fetch": nothingRecorded,
  "origin-null": nothingRecorded,
  "get-does-not-run-actions": nothingRecorded,
  "put-and-delete-refused": nothingRecorded,
  "forwarded-host-matching-origin": (o) => (o.recorded === 1 ? null : `recorded ${o.recorded}, expected 1`),
  "forwarded-to-another-worker": ledgerIs("fwd"),
  "mutation-once": (o) => (o.status === 200 && o.body ? ledgerIs("only")(o) : `status ${o.status}, body ${o.body}`),
  "concurrent-mutations": (o) => (o.answered === 64 && o.recorded === 64 && o.distinct === 64 ? null : JSON.stringify(o)),
  "concurrent-identity": (o) => (o.n === 48 && o.leaks === 0 ? null : JSON.stringify(o)),
  "disconnect-after-commit": ledgerIs("a"),
  "disconnect-before-commit": ledgerIs("b"),
};
for (const name of Object.keys(EXPECT)) if (!cases[name]) throw new Error(`no case ${name}`);

const results = [];
let failures = 0;
for (const [name, { kind, fn }] of Object.entries(cases)) {
  if (only && !only.has(name)) continue;
  let verdict;
  let detail;
  try {
    if (kind === "compare" || kind === "known") {
      comparing = true;
      const b = await fn(baseline, "b");
      const c = await fn(candidate, "c");
      comparing = false;
      const same = JSON.stringify(b) === JSON.stringify(c);
      verdict = same ? "MATCH" : kind === "known" ? "KNOWN-DIFF" : "DIFF";
      detail = { baseline: b, candidate: c, ...(kind === "known" ? { note: cases[name].note } : {}) };
      if (EXPECT[name]) {
        const control = process.env.NEXTRS_NEGATIVE_CONTROL ?? "";
        const [defect, sides] = control.split(":");
        if (defect) {
          corrupt(defect, c);
          if (sides === "both") corrupt(defect, b);
        }
        const oracleWrong = EXPECT[name](b);
        const candidateWrong = EXPECT[name](c);
        if (oracleWrong) verdict = "ORACLE-FAIL";
        else if (candidateWrong) verdict = "FAIL";
        else if (defect && JSON.stringify(b) !== JSON.stringify(c)) verdict = "DIFF";
        Object.assign(detail, { asserted: true, oracleWrong, candidateWrong });
      }
    } else {
      detail = { candidate: await fn() };
      verdict = "OBSERVED";
    }
  } catch (error) {
    comparing = false;
    verdict = "ERROR";
    detail = { error: String(error?.stack ?? error).split("\n").slice(0, 3).join(" | ") };
  }
  if (["DIFF", "ERROR", "FAIL", "ORACLE-FAIL"].includes(verdict)) failures++;
  results.push({ name, kind, verdict, ...detail });
  const show = verdict === "MATCH" ? JSON.stringify(detail.baseline).slice(0, 140) : JSON.stringify(detail).slice(0, 900);
  console.log(`${verdict.padEnd(9)} ${name.padEnd(32)} ${show}`);
}
if (!(await healthy())) {
  failures++;
  console.log("FAIL      candidate unhealthy after the suite");
}
if (args.json) writeFileSync(args.json, JSON.stringify({ run, ids, results }, null, 2));
const count = (v) => results.filter((r) => r.verdict === v).length;
console.log(`${count("MATCH")} match (${results.filter((r) => r.verdict === "MATCH" && r.asserted).length} asserted), ${count("KNOWN-DIFF")} known divergences, ${count("OBSERVED")} observed, ${failures} failing`);
process.exit(failures ? 1 : 0);
