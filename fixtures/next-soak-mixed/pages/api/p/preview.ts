import type { NextApiRequest, NextApiResponse } from "next";

export default function handler(req: NextApiRequest, res: NextApiResponse) {
  res.setPreviewData({ who: String(req.query.who ?? "soak") });
  res.status(200).json({ preview: true });
}
