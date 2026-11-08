import type { NextApiRequest, NextApiResponse } from "next";

// What the API route saw: method, query, parsed body, cookies and a few
// headers (Next's default body parser: JSON, urlencoded forms, text).
export default function handler(req: NextApiRequest, res: NextApiResponse) {
  res.setHeader("x-api-route", "echo");
  res.status(200).json({
    method: req.method,
    query: req.query,
    body: req.body ?? null,
    bodyType: typeof req.body,
    cookies: req.cookies,
    contentType: req.headers["content-type"] ?? null,
    user: req.headers["x-test-user"] ?? null,
    url: req.url,
  });
}
