import { headers } from "next/headers";
import { rejection, verifyAccessToken } from "../../lib/tokens";

export const dynamic = "force-dynamic";

// A resource server's handler verifies the token itself (the proxy's check
// is not its authorization) and answers with the verified identity.
export async function GET(req: Request) {
  const result = await verifyAccessToken(req.headers.get("authorization"));
  if (!result.ok) return rejection(result);
  return Response.json({
    sub: result.claims.sub,
    name: result.claims.name ?? null,
    iss: result.claims.iss,
    aud: result.claims.aud,
    alg: result.header.alg,
    kid: result.header.kid ?? null,
    proxySub: (await headers()).get("x-auth-sub"),
  });
}
