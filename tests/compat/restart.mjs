// Data cache persistence across a server restart, compared between
// `next start` (which persists to .next/cache/fetch-cache) and NextRS
// (with --cache-dir). Run --phase prepare, restart both servers, then
// --phase verify with the same --ns.
//
//   node tests/compat/restart.mjs --phase prepare|verify --ns NAME \
//        --baseline URL --candidate URL --upstream URL [--json out.json]
//
// Compared: a force-cache fetch and an unstable_cache entry both survive,
// and so does an ISR page generated on demand before the restart (next
// start keeps it under .next/server/route-cache, NextRS in --cache-dir): it
// is served afterwards as the same generation, from the cache.
// Reported only: a tag revalidated before the restart. next start keeps its
// tag manifest in memory, so it serves the old entry again; NextRS
// persists tags and refetches.
import http from "node:http";
import { readFileSync, writeFileSync } from "node:fs";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((pairs, arg, i, all) => {
    if (arg.startsWith("--")) pairs.push([arg.slice(2), all[i + 1]]);
    return pairs;
  }, []),
);
const servers = { baseline: args.baseline, candidate: args.candidate };

function get(base, path, headers = {}) {
  return new Promise((resolve) => {
    http
      .get(new URL(path, base), { headers }, (res) => {
        const chunks = [];
        res.on("data", (c) => chunks.push(c));
        res.on("end", () => resolve({ status: res.statusCode, body: Buffer.concat(chunks).toString() }));
      })
      .on("error", (e) => resolve({ error: e.code }));
  });
}

const hit = async (base, query) => JSON.parse((await get(base, `/api/data?${query}`)).body).body.hit;
const isrPage = async (base, slug) => {
  const r = await new Promise((resolve) => {
    http.get(new URL(`/isr/${slug}`, base), (res) => {
      const chunks = [];
      res.on("data", (c) => chunks.push(c));
      res.on("end", () => resolve({ cache: res.headers["x-nextjs-cache"] ?? null, body: Buffer.concat(chunks).toString() }));
    }).on("error", (e) => resolve({ error: e.code }));
  });
  return { cache: r.cache, stamp: /stamp-(?:<!-- -->)?(\d+)/.exec(r.body ?? "")?.[1] ?? null };
};
const creator = async (base, key, id) => {
  const r = await get(base, "/isolation/persistent", { "x-nextrs-key": key, "x-nextrs-id": id });
  return /createdBy=(?:<!-- -->)?([^<\s]+)/.exec(r.body)?.[1] ?? null;
};

async function observe(base, ns, phase) {
  const forced = `key=${ns}-persist&cache=force-cache`;
  const tagged = `key=${ns}-ptag&revalidate=3600&tag=${ns}-t`;
  if (phase === "prepare") {
    const result = {
      forced: await hit(base, forced),
      tagged: await hit(base, tagged),
      creator: await creator(base, `${ns}-uc`, "before-restart"),
      isr: await isrPage(base, `rs-${ns}`),
    };
    await get(base, `/api/revalidate?tag=${ns}-t`);
    return result;
  }
  return {
    forced: await hit(base, forced),
    tagged: await hit(base, tagged),
    creator: await creator(base, `${ns}-uc`, "after-restart"),
    isr: await isrPage(base, `rs-${ns}`),
  };
}

const observed = {};
for (const [which, base] of Object.entries(servers)) {
  observed[which] = await observe(base, `${args.ns}-${which[0]}`, args.phase);
}
console.log(JSON.stringify(observed));
if (args.phase === "prepare") {
  if (args.json) writeFileSync(args.json, JSON.stringify(observed, null, 2));
  process.exit(0);
}
const prepared = args.prepared ? JSON.parse(readFileSync(args.prepared, "utf8")) : null;
// The ISR page: served from the cache, and the generation from before.
const isrKept = (which) => ({ cache: observed[which].isr.cache, sameGeneration: observed[which].isr.stamp !== null && observed[which].isr.stamp === prepared?.[which]?.isr?.stamp });
const pick = (which) => ({ forced: observed[which].forced, creator: observed[which].creator, isr: isrKept(which) });
const same = JSON.stringify(pick("baseline")) === JSON.stringify(pick("candidate"));
const persisted = observed.candidate.forced === 1 && observed.candidate.creator === "before-restart" && isrKept("candidate").sameGeneration;
console.log(`${same && persisted ? "MATCH" : "DIFF "}  persistence  next=${JSON.stringify(observed.baseline)} nextrs=${JSON.stringify(observed.candidate)}`);
console.log(`REPORT tag revalidated before restart: next=${observed.baseline.tagged} nextrs=${observed.candidate.tagged} (1 = old entry served)`);
if (args.json) {
  writeFileSync(args.json, JSON.stringify({ before: prepared, after: observed, same, persisted }, null, 2));
}
process.exitCode = same && persisted ? 0 : 1;
