// Runs in the probe container (host PID and cgroup namespaces; see lib.mjs).
// Samples one container from outside, so sampling costs the measured
// container no CPU and no memory:
//
//   node /re/sampler.mjs CGROUP_DIR PID OUT.jsonl [INTERVAL_MS] [SMAPS_EVERY]
//
// Every INTERVAL_MS (default 1000): the cgroup's memory and CPU files and
// the main process's /proc status; every SMAPS_EVERY-th sample (default 5)
// also /proc/PID/smaps_rollup (PSS needs CAP_SYS_PTRACE). One JSON object
// per line. Stops when the cgroup disappears (container removed) or on
// SIGTERM.
import { appendFileSync, readFileSync } from "node:fs";

const [cg, pid, out, intervalArg, smapsArg] = process.argv.slice(2);
const interval = Number(intervalArg ?? 1000);
const smapsEvery = Number(smapsArg ?? 5);

const read = (path) => {
  try {
    return readFileSync(path, "utf8");
  } catch {
    return null;
  }
};
const kv = (text, keys) => {
  const o = {};
  for (const line of (text ?? "").split("\n")) {
    const [k, v] = line.trim().split(/\s+/);
    if (keys === null || keys.includes(k)) o[k] = Number(v);
  }
  return o;
};
const kib = (text, keys) => {
  const o = {};
  for (const line of (text ?? "").split("\n")) {
    const m = line.match(/^([A-Za-z_]+):\s+(\d+)(?:\s+kB)?/);
    if (m && keys.includes(m[1])) o[m[1]] = Number(m[2]) * (line.includes("kB") ? 1024 : 1);
  }
  return o;
};

let n = 0;
let stop = false;
process.on("SIGTERM", () => (stop = true));
while (!stop) {
  const current = read(`${cg}/memory.current`);
  if (current === null) break;
  const s = {
    t: Date.now(),
    memory_current: Number(current),
    memory_peak: Number(read(`${cg}/memory.peak`)),
    events: kv(read(`${cg}/memory.events`), null),
    stat: kv(read(`${cg}/memory.stat`), ["anon", "file", "kernel", "kernel_stack", "pagetables", "sock", "shmem",
      "file_mapped", "slab", "inactive_file", "active_file", "inactive_anon", "active_anon"]),
    cpu: kv(read(`${cg}/cpu.stat`), ["usage_usec", "user_usec", "system_usec", "nr_periods", "nr_throttled", "throttled_usec"]),
    pressure: (read(`${cg}/memory.pressure`) ?? "").split("\n")[0],
    proc: kib(read(`/proc/${pid}/status`), ["VmRSS", "VmHWM", "VmSize", "RssAnon", "RssFile", "RssShmem", "Threads",
      "voluntary_ctxt_switches", "nonvoluntary_ctxt_switches"]),
  };
  if (n % smapsEvery === 0) {
    s.smaps = kib(read(`/proc/${pid}/smaps_rollup`), ["Rss", "Pss", "Pss_Anon", "Pss_File", "Anonymous", "Private_Dirty",
      "Shared_Clean", "Swap"]);
  }
  appendFileSync(out, JSON.stringify(s) + "\n");
  n++;
  await new Promise((r) => setTimeout(r, interval));
}
