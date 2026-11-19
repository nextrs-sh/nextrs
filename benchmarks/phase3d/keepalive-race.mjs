// Reproduces the keep-alive race behind the Phase 2G/3C soak resets against a
// running NextRS server: on N connections at once, one request, then a
// second request on the same socket after an idle offset spread around the
// server's 30 s limit. Records, per offset, whether the second request was
// answered, reset, or found the connection already closed.
//
//   node benchmarks/phase3d/keepalive-race.mjs --port 3000 [--path /] [--from 29700 --to 30300 --step 25]
import net from "node:net";
import { writeFileSync } from "node:fs";

const argv = process.argv.slice(2);
const opt = (n, d) => (argv.includes(`--${n}`) ? argv[argv.indexOf(`--${n}`) + 1] : d);
const port = Number(opt("port", 3000));
const path = opt("path", "/");
const from = Number(opt("from", 29700)), to = Number(opt("to", 30300)), step = Number(opt("step", 25));
const req = `GET ${path} HTTP/1.1\r\nhost: 127.0.0.1:${port}\r\nconnection: keep-alive\r\n\r\n`;

function probe(offset) {
  return new Promise((resolve) => {
    const s = net.connect(port, "127.0.0.1");
    let phase = 0, buf = "", t1 = 0, secondSentAt = 0, closedBeforeSend = false;
    const done = (outcome) => { s.destroy(); resolve({ offset, outcome, closedBeforeSend, sinceFirstMs: secondSentAt ? secondSentAt - t1 : null }); };
    s.on("connect", () => s.write(req));
    s.on("data", (d) => {
      buf += d.toString("latin1");
      if (phase === 0 && /\r\n\r\n/.test(buf)) {
        // First response complete enough (headers; the bodies here are small and arrive with them).
        phase = 1; t1 = Date.now(); buf = "";
        setTimeout(() => {
          if (s.destroyed || s.readyState !== "open") { closedBeforeSend = true; return done("closed-before-send"); }
          secondSentAt = Date.now();
          s.write(req);
        }, offset);
      } else if (phase === 1 && secondSentAt && /^HTTP\/1\.1 \d{3}/.test(buf)) done(`answered ${buf.slice(9, 12)}`);
    });
    s.on("end", () => { if (phase === 1 && !secondSentAt) closedBeforeSend = true; if (secondSentAt) done("eof-after-send"); });
    s.on("error", (e) => done(secondSentAt ? `${e.code}-after-send` : `${e.code}-before-send`));
    s.on("close", () => { if (phase === 1 && !secondSentAt) { /* wait for the timer to report */ } });
  });
}

const offsets = [];
for (let o = from; o <= to; o += step) offsets.push(o);
const results = await Promise.all(offsets.map(probe));
const summary = {};
for (const r of results) summary[r.outcome] = (summary[r.outcome] ?? 0) + 1;
console.log(JSON.stringify({ port, path, summary, results }, null, 1));
if (opt("json")) writeFileSync(opt("json"), JSON.stringify({ port, path, summary, results }, null, 2));
