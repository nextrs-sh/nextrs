import type { NextApiRequest, NextApiResponse } from "next";

export default function handler(req: NextApiRequest, res: NextApiResponse) {
  res.status(200).json({ method: req.method, token: req.headers["x-soak-token"] ?? null });
}
