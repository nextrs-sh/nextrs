import { createRemoteJWKSet, jwtVerify, type JWTPayload, type JWTHeaderParameters } from "jose";
import { AUDIENCE, PROVIDER_ALGORITHMS, issuerFor } from "./config";

// One remote key set per issuer and isolate, as jose's documentation shows
// (module scope). Short cache times so the suites can rotate keys:
// cooldownDuration is how soon an unknown `kid` may refetch, cacheMaxAge
// how long a fetched set counts as fresh.
const keySets = new Map<string, ReturnType<typeof createRemoteJWKSet>>();
function keySet(issuer: string) {
  let set = keySets.get(issuer);
  if (!set) {
    set = createRemoteJWKSet(new URL(`${issuer}/jwks`), { cooldownDuration: 200, cacheMaxAge: 1500, timeoutDuration: 1000 });
    keySets.set(issuer, set);
  }
  return set;
}

export type Verified = { ok: true; claims: JWTPayload; header: JWTHeaderParameters; tenant: string };
export type Rejected = { ok: false; status: 401 | 503; error: string; code: string; claim?: string };

// Token problems are 401; a provider that cannot be reached or answers
// badly is 503 (the request might succeed later), never an acceptance.
const TOKEN_ERRORS = new Set([
  "ERR_JWT_EXPIRED",
  "ERR_JWT_CLAIM_VALIDATION_FAILED",
  "ERR_JWT_INVALID",
  "ERR_JWS_INVALID",
  "ERR_JWS_SIGNATURE_VERIFICATION_FAILED",
  "ERR_JOSE_ALG_NOT_ALLOWED",
  "ERR_JWKS_NO_MATCHING_KEY",
  "ERR_JWKS_MULTIPLE_MATCHING_KEYS",
  "ERR_JOSE_NOT_SUPPORTED",
]);

export async function verifyAccessToken(authorization: string | null, tenant = "main"): Promise<Verified | Rejected> {
  const issuer = issuerFor(tenant);
  if (!issuer) return { ok: false, status: 401, error: "unknown_tenant", code: "unknown-tenant" };
  const token = /^Bearer ([A-Za-z0-9._~+/=-]+)$/.exec(authorization ?? "")?.[1];
  if (!token) return { ok: false, status: 401, error: "invalid_request", code: "no-bearer-token" };
  try {
    const { payload, protectedHeader } = await jwtVerify(token, keySet(issuer), {
      issuer,
      audience: AUDIENCE,
      algorithms: PROVIDER_ALGORITHMS,
      requiredClaims: ["sub", "exp"],
    });
    return { ok: true, claims: payload, header: protectedHeader, tenant };
  } catch (error) {
    // DOMException codes are numbers (an AbortError's is 20).
    const raw = (error as { code?: unknown })?.code;
    const code = typeof raw === "string" ? raw : "";
    if (TOKEN_ERRORS.has(code)) {
      const claim = (error as { claim?: string }).claim;
      return { ok: false, status: 401, error: "invalid_token", code, ...(claim ? { claim } : {}) };
    }
    if (code.startsWith("ERR_J")) return { ok: false, status: 503, error: "provider_unavailable", code };
    // Not jose's: the fetch itself failed (refused, denied, reset).
    console.error("JWKS fetch failed:", (error as Error)?.message);
    return { ok: false, status: 503, error: "provider_unavailable", code: "fetch-failed" };
  }
}

export function rejection(result: Rejected) {
  const body = { error: result.error, code: result.code, ...(result.claim ? { claim: result.claim } : {}) };
  return Response.json(body, { status: result.status, headers: { "www-authenticate": `Bearer error="${result.error}"` } });
}
