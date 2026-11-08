import type { NextApiRequest, NextApiResponse } from "next";

// Enables Preview Mode when the secret matches (never echoes it).
export default function handler(req: NextApiRequest, res: NextApiResponse) {
  if (req.query.secret !== "preview-secret") return res.status(401).json({ message: "invalid token" });
  res.setPreviewData({ user: String(req.query.user ?? "editor") }, { maxAge: 600 });
  res.status(200).json({ preview: true });
}
