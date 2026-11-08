import type { NextApiRequest, NextApiResponse } from "next";

// Busy CPU for ?ms= milliseconds (at most 30 s) without yielding: with a
// short --js-stall-timeout NextRS terminates and replaces the isolate.
export default function handler(req: NextApiRequest, res: NextApiResponse) {
  const until = Date.now() + Math.min(Number(req.query.ms ?? 0), 30_000);
  let spins = 0;
  while (Date.now() < until) spins++;
  res.status(200).json({ spins: spins > 0 });
}
