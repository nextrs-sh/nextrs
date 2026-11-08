import type { NextApiRequest, NextApiResponse } from "next";

export default function handler(_req: NextApiRequest, res: NextApiResponse) {
  res.clearPreviewData();
  res.status(200).json({ preview: false });
}
