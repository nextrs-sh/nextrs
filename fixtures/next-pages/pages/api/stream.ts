import type { NextApiRequest, NextApiResponse } from "next";

const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));

// A streamed response: chunks written over time with res.write.
export default async function handler(req: NextApiRequest, res: NextApiResponse) {
  const chunks = Math.min(Number(req.query.chunks ?? 5), 1000);
  const gap = Math.min(Number(req.query.gap ?? 20), 5000);
  res.setHeader("content-type", "text/plain; charset=utf-8");
  res.status(200);
  for (let i = 0; i < chunks; i++) {
    res.write(`chunk-${i}\n`);
    await delay(gap);
  }
  res.end("done\n");
}
