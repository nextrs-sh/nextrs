import type { NextApiRequest, NextApiResponse } from "next";

// A small body-parser limit: larger bodies get Next's 413.
export const config = { api: { bodyParser: { sizeLimit: "1kb" } } };

export default function handler(req: NextApiRequest, res: NextApiResponse) {
  res.status(200).json({ received: JSON.stringify(req.body ?? null).length });
}
