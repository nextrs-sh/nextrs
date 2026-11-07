import { rejection, verifyAccessToken } from "../../../../lib/tokens";

export const dynamic = "force-dynamic";

// A multi-tenant API: the path picks the tenant's provider.
export async function GET(req: Request, { params }: { params: Promise<{ tenant: string }> }) {
  const { tenant } = await params;
  const result = await verifyAccessToken(req.headers.get("authorization"), tenant);
  if (!result.ok) return rejection(result);
  return Response.json({ tenant, sub: result.claims.sub });
}
