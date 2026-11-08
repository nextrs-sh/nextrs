import type { NextApiRequest, NextApiResponse } from "next";

export default function handler(req: NextApiRequest, res: NextApiResponse) {
  res.setHeader("Set-Cookie", [`api-a=${req.query.v ?? "1"}; Path=/`, "api-b=2; Path=/; HttpOnly; SameSite=Lax"]);
  res.setHeader("x-custom", ["one", "two"]);
  res.status(201).json({ seen: req.cookies });
}
