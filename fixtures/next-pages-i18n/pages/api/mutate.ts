import type { NextApiRequest, NextApiResponse } from "next";
import { upstream } from "../../lib/upstream";

// One side effect per POST, recorded outside every isolate.
export default async function handler(req: NextApiRequest, res: NextApiResponse) {
  if (req.method !== "POST") {
    res.setHeader("allow", "POST");
    res.status(405).end();
    return;
  }
  const key = String(req.query.key ?? "i18n-mutate");
  const id = String(req.query.id ?? "0");
  const ledger = await fetch(`${upstream()}/mutations?key=${encodeURIComponent(key)}&id=${encodeURIComponent(id)}`, { method: "POST" }).then((r) => r.json());
  res.status(200).json({ ok: true, count: ledger.count });
}
