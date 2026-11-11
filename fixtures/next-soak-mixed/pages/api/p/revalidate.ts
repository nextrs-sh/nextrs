import type { NextApiRequest, NextApiResponse } from "next";

export default async function handler(_req: NextApiRequest, res: NextApiResponse) {
  await res.revalidate("/p/isr");
  res.status(200).json({ revalidated: true });
}
