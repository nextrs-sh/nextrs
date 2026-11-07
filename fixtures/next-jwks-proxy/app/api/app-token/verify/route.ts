import { decodeProtectedHeader, jwtVerify } from "jose";
import { verificationKey } from "../../../lib/session";

export const dynamic = "force-dynamic";
const ALGS = ["ES256", "PS256", "RS256", "EdDSA"];

// Verifies a token signed with one of the application's keys; the key is
// chosen by `alg` among the allowed ones, and jwtVerify is told the same
// algorithm, so a header cannot pick another verification.
export async function POST(req: Request) {
  const token = /^Bearer (.+)$/.exec(req.headers.get("authorization") ?? "")?.[1] ?? "";
  try {
    const alg = decodeProtectedHeader(token).alg ?? "";
    if (!ALGS.includes(alg)) return Response.json({ ok: false, code: "alg-not-allowed" }, { status: 401 });
    const { payload } = await jwtVerify(token, await verificationKey(alg), {
      algorithms: [alg],
      issuer: "nextrs-jwks-app",
      audience: "nextrs-services",
    });
    return Response.json({ ok: true, sub: payload.sub, alg });
  } catch (error) {
    return Response.json({ ok: false, code: (error as { code?: string })?.code ?? "error" }, { status: 401 });
  }
}
