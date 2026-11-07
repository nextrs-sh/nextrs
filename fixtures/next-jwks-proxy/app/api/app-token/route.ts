import { SignJWT } from "jose";
import { currentSession } from "../../lib/dal";
import { signingKey } from "../../lib/session";

export const dynamic = "force-dynamic";
const ALGS = ["ES256", "PS256", "RS256", "EdDSA"];

// Issues a short-lived token for the signed-in user, signed with the
// application's key for `alg` (what an application hands to its own
// services). The suites verify it with node:crypto.
export async function GET(req: Request) {
  const session = await currentSession();
  if (!session) return Response.json({ error: "unauthorized" }, { status: 401 });
  const alg = new URL(req.url).searchParams.get("alg") ?? "ES256";
  if (!ALGS.includes(alg)) return Response.json({ error: "unsupported alg" }, { status: 400 });
  const token = await new SignJWT({ name: session.name })
    .setProtectedHeader({ alg, kid: `app-${alg}` })
    .setSubject(session.userId)
    .setIssuer("nextrs-jwks-app")
    .setAudience("nextrs-services")
    .setIssuedAt()
    .setExpirationTime("5m")
    .sign(await signingKey(alg));
  return Response.json({ token });
}
