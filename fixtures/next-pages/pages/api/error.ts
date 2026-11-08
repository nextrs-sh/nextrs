import type { NextApiRequest, NextApiResponse } from "next";

export default function handler(req: NextApiRequest, res: NextApiResponse) {
  if (req.query.kind === "status") return res.status(418).json({ error: "teapot" });
  throw new Error("API route failure (synthetic)");
}
