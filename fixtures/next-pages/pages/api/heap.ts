import type { NextApiRequest, NextApiResponse } from "next";

// Allocates until the isolate's heap limit is reached (NextRS replaces the
// isolate; next start would crash the process): only ever called by the
// reliability suite against NextRS with --js-heap-limit.
export default function handler(_req: NextApiRequest, res: NextApiResponse) {
  const hoard: string[][] = [];
  for (;;) hoard.push(Array.from({ length: 100_000 }, (_, i) => `entry-${i}-${hoard.length}`));
  res.status(200).end();
}
