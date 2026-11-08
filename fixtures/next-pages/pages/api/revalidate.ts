import type { NextApiRequest, NextApiResponse } from "next";

// On-demand revalidation of a Pages Router SSG page.
export default async function handler(req: NextApiRequest, res: NextApiResponse) {
  const path = String(req.query.path ?? "/on-demand");
  try {
    await res.revalidate(path);
    res.status(200).json({ revalidated: true, path });
  } catch (error) {
    res.status(500).json({ revalidated: false, path, error: String((error as Error).message) });
  }
}
