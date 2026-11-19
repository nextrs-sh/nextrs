// Timer latency probe (Phase 3C). The same code runs under Node.js
// (`node -e "require('./timer-probe.js').run().then(r => console.log(JSON.stringify(r)))"`)
// and inside a NextRS isolate (timers.js), so the two can be compared on one
// machine. Each case reports the delay actually observed (ms, from
// performance.now()) beyond nothing: the scheduled delay is not subtracted.
"use strict";

const now = () => performance.now();
const stats = (xs) => {
  const s = [...xs].sort((a, b) => a - b);
  const q = (p) => s[Math.min(s.length - 1, Math.floor(p * s.length))];
  const r = (x) => Math.round(x * 1000) / 1000;
  return { n: s.length, p50: r(q(0.5)), p90: r(q(0.9)), p99: r(q(0.99)), max: r(s[s.length - 1]), mean: r(s.reduce((a, b) => a + b, 0) / s.length) };
};
const timeout = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const immediate = () => new Promise((resolve) => setImmediate(resolve));

async function each(n, fn) {
  const out = [];
  for (let i = 0; i < n; i++) {
    const t0 = now();
    await fn();
    out.push(now() - t0);
  }
  return stats(out);
}

function spin(ms) {
  const until = now() + ms;
  while (now() < until) {}
}

async function run({ n = 400 } = {}) {
  const result = {};
  // Warm up the JIT and the loop.
  await each(50, () => timeout(0));
  result.microtask = await each(n, () => Promise.resolve());
  result.setImmediate = await each(n, immediate);
  result.setTimeout0 = await each(n, () => timeout(0));
  result.setTimeout1 = await each(n, () => timeout(1));
  result.setTimeout5 = await each(Math.min(n, 200), () => timeout(5));
  // Five nested setTimeout(0) calls: per chain.
  result.nested5x0 = await each(Math.min(n, 200), async () => {
    for (let i = 0; i < 5; i++) await timeout(0);
  });
  // 100 concurrent setTimeout(1): until the last fires.
  result.concurrent100x1 = await each(100, () => Promise.all(Array.from({ length: 100 }, () => timeout(1))));
  // Cancellation: 200 timers, every other one cleared; none of the cleared
  // may fire, all of the others must.
  {
    let fired = 0, wrong = 0;
    const handles = [];
    const all = [];
    for (let i = 0; i < 200; i++) {
      const cancel = i % 2 === 0;
      all.push(new Promise((resolve) => {
        handles.push(setTimeout(() => { if (cancel) wrong++; else fired++; resolve(); }, 2));
        if (cancel) resolve();
      }));
    }
    for (let i = 0; i < 200; i += 2) clearTimeout(handles[i]);
    await Promise.all(all);
    await timeout(10);
    result.cancellation = { expected: 100, fired, firedAfterCancel: wrong };
  }
  // setTimeout(1) armed before 3 ms of synchronous work (the timer cannot
  // fire before the work ends; the overshoot after it is the cost).
  result.behindCpu3ms = await each(100, async () => {
    const p = timeout(1);
    spin(3);
    await p;
  });
  // setTimeout(0) while 4 other chains keep the loop busy with short
  // synchronous slices (0.2 ms) between timers.
  {
    let stop = false;
    const busy = async () => { while (!stop) { spin(0.2); await timeout(0); } };
    const workers = [busy(), busy(), busy(), busy()];
    result.setTimeout0UnderLoad = await each(Math.min(n, 200), () => timeout(0));
    stop = true;
    await Promise.all(workers);
  }
  return result;
}

module.exports = { run };
