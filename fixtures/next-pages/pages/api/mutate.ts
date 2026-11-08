import type { NextApiRequest, NextApiResponse } from "next";

const upstream = () => process.env.NEXTRS_UPSTREAM ?? "http://127.0.0.1:4999";

// A mutation: one POST records exactly one side effect in the upstream
// ledger (outside every isolate). Replays would record it twice.
export default async function handler(req: NextApiRequest, res: NextApiResponse) {
  if (req.method !== "POST") {
    res.setHeader("Allow", "POST");
    return res.status(405).end();
  }
  const key = String(req.query.key ?? "pages-mutate");
  const id = String(req.query.id ?? "1");
  const ms = Math.min(Number(req.query.ms ?? 0), 20_000);
  const r = await fetch(`${upstream()}/mutations?key=${encodeURIComponent(key)}&id=${encodeURIComponent(id)}`, { method: "POST" });
  if (ms) await new Promise((resolve) => setTimeout(resolve, ms));
  res.status(200).json({ recorded: (await r.json()).count });
}
