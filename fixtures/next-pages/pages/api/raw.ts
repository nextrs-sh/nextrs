import type { NextApiRequest, NextApiResponse } from "next";

// bodyParser disabled: the route reads the raw request stream itself.
export const config = { api: { bodyParser: false } };

export default async function handler(req: NextApiRequest, res: NextApiResponse) {
  let length = 0;
  const hash = { sum: 0 };
  for await (const chunk of req) {
    length += chunk.length;
    for (const byte of chunk as Buffer) hash.sum = (hash.sum + byte) % 65521;
  }
  res.status(200).json({ method: req.method, length, sum: hash.sum });
}
