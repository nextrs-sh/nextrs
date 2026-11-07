import { SignJWT, importPKCS8, importSPKI, jwtVerify, type JWTPayload } from "jose";
import { appKeys } from "./config";

// The application's own session: an ES256 JWT it signs with its private
// key and verifies with the public key, in an httpOnly cookie.
export const SESSION_COOKIE = "session";
const ISSUER = "nextrs-jwks-app";

export type SessionPayload = JWTPayload & { userId: string; name: string };

// Imported once per isolate and algorithm.
const privateKeys = new Map<string, Promise<CryptoKey>>();
const publicKeys = new Map<string, Promise<CryptoKey>>();
export function signingKey(alg: string) {
  if (!privateKeys.has(alg)) privateKeys.set(alg, importPKCS8(appKeys()[alg].private, alg));
  return privateKeys.get(alg)!;
}
export function verificationKey(alg: string) {
  if (!publicKeys.has(alg)) publicKeys.set(alg, importSPKI(appKeys()[alg].public, alg));
  return publicKeys.get(alg)!;
}

export async function encrypt(payload: { userId: string; name: string }, expires: Date) {
  return new SignJWT(payload)
    .setProtectedHeader({ alg: "ES256" })
    .setIssuer(ISSUER)
    .setAudience(ISSUER)
    .setIssuedAt()
    .setExpirationTime(expires)
    .sign(await signingKey("ES256"));
}

// null for a missing, malformed, tampered, wrongly signed or expired token.
export async function decrypt(session: string | undefined = ""): Promise<SessionPayload | null> {
  if (!session) return null;
  try {
    const { payload } = await jwtVerify(session, await verificationKey("ES256"), { algorithms: ["ES256"], issuer: ISSUER, audience: ISSUER });
    if (typeof payload.userId !== "string" || typeof payload.name !== "string") return null;
    return payload as SessionPayload;
  } catch (error) {
    const code = (error as { code?: string })?.code;
    if (!code?.startsWith("ERR_J")) console.error("session verification failed:", error);
    return null;
  }
}
