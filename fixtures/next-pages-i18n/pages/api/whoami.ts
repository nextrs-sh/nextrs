import type { NextApiRequest, NextApiResponse } from "next";
import { cookieValue } from "../../lib/upstream";

// API routes have no locale; this echoes what reached the handler.
export default function handler(req: NextApiRequest, res: NextApiResponse) {
  res.status(200).json({
    method: req.method,
    url: req.url,
    query: req.query,
    user: cookieValue(req.headers.cookie, "user"),
    proxy: req.headers["x-proxy-seen"] ?? null,
  });
}
