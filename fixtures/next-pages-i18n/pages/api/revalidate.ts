import type { NextApiRequest, NextApiResponse } from "next";

// On-demand revalidation of a localized path (/fr/isr, /isr, ...).
export default async function handler(req: NextApiRequest, res: NextApiResponse) {
  const path = String(req.query.path ?? "/isr");
  try {
    await res.revalidate(path);
    res.status(200).json({ revalidated: path });
  } catch (error) {
    res.status(500).json({ error: String((error as Error).message) });
  }
}
