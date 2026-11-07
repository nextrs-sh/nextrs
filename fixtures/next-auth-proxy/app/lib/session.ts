import { SignJWT, jwtVerify, type JWTPayload } from "jose";

// The Next.js authentication guide's stateless session: an HS256 JWT in an
// httpOnly cookie. The secret comes from the environment at run time.
export const SESSION_COOKIE = "session";

export type SessionPayload = JWTPayload & { userId: string; name: string };

function key() {
  const secret = process.env.SESSION_SECRET;
  if (!secret) throw new Error("SESSION_SECRET is not set");
  return new TextEncoder().encode(secret);
}

export async function encrypt(payload: { userId: string; name: string }, expires: Date) {
  return new SignJWT(payload)
    .setProtectedHeader({ alg: "HS256" })
    .setIssuedAt()
    .setExpirationTime(expires)
    .sign(key());
}

// null for a missing, malformed, tampered, wrongly signed or expired token.
export async function decrypt(session: string | undefined = ""): Promise<SessionPayload | null> {
  if (!session) return null;
  try {
    const { payload } = await jwtVerify(session, key(), { algorithms: ["HS256"] });
    if (typeof payload.userId !== "string" || typeof payload.name !== "string") return null;
    return payload as SessionPayload;
  } catch (error) {
    const code = (error as { code?: string })?.code;
    if (!code?.startsWith("ERR_J")) console.error("session verification failed:", error);
    return null;
  }
}
