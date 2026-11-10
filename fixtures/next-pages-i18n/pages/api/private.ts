import type { NextApiRequest, NextApiResponse } from "next";
import { cookieValue } from "../../lib/upstream";

const SESSION = "synthetic-valid-session";

// Authorizes itself; never relies on the proxy.
export default function handler(req: NextApiRequest, res: NextApiResponse) {
  if (cookieValue(req.headers.cookie, "session") !== SESSION) {
    res.status(401).json({ error: "unauthorized" });
    return;
  }
  res.status(200).json({ secret: "api-secret" });
}
