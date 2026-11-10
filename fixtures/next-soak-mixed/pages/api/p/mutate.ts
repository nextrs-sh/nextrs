import type { NextApiRequest, NextApiResponse } from "next";
import { upstream } from "../../../lib/upstream";

// One side effect per POST, recorded outside every isolate: the soak checks
// that no mutation ran twice.
export default async function handler(req: NextApiRequest, res: NextApiResponse) {
  if (req.method !== "POST") {
    res.status(405).end();
    return;
  }
  const id = String(req.query.id);
  const r = await fetch(`${upstream()}/mutations?key=soak&id=${encodeURIComponent(id)}`, { method: "POST" }).then((x) => x.json());
  res.status(200).json({ id, count: r.count });
}
