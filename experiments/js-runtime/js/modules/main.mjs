import { answer, later } from "./dep.mjs";

const value = await later(5);
globalThis.__report(JSON.stringify({ ok: answer === 42 && value === "resolved", answer, value, topLevelAwait: true }));
