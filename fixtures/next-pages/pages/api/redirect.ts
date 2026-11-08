import type { NextApiRequest, NextApiResponse } from "next";

export default function handler(req: NextApiRequest, res: NextApiResponse) {
  if (req.query.status === "301") return res.redirect(301, "/about");
  res.redirect("/books/1?via=api");
}
