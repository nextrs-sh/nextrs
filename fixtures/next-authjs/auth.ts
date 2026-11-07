import NextAuth from "next-auth";
import Credentials from "next-auth/providers/credentials";
import { USERS } from "./app/lib/users";

// Pages behind the proxy's check (the `authorized` callback).
const protectedPages = ["/dashboard", "/reports", "/actions/protected"];

export const { handlers, auth, signIn, signOut } = NextAuth({
  // The test runner sets AUTH_SECRET; next start and NextRS listen on
  // loopback addresses, so the request's host is trusted.
  trustHost: true,
  session: { strategy: "jwt", maxAge: 60 * 60 },
  providers: [
    Credentials({
      credentials: { username: { label: "Username" }, password: { label: "Password", type: "password" } },
      authorize: async (credentials) => {
        const user = USERS[String(credentials?.username ?? "")];
        if (!user || user.password !== credentials?.password) return null;
        return { id: user.userId, name: user.name, email: user.email };
      },
    }),
    // A generic OpenID Connect provider: the local test provider
    // (tests/compat/oidc-stub.mjs), with PKCE, state and nonce checks.
    {
      id: "local-oidc",
      name: "Local OpenID provider",
      type: "oidc",
      issuer: process.env.OIDC_ISSUER,
      clientId: process.env.OIDC_CLIENT_ID,
      clientSecret: process.env.OIDC_CLIENT_SECRET,
      checks: ["pkce", "state", "nonce"],
      authorization: { params: { scope: "openid profile email" } },
    },
  ],
  callbacks: {
    // Without a database adapter Auth.js gives a provider's user a new
    // random id per sign-in; keep the provider's subject instead (the
    // `jwt` callback's documented use).
    jwt({ token, account, profile }) {
      if (account?.provider === "local-oidc" && typeof profile?.sub === "string") token.sub = profile.sub;
      return token;
    },
    authorized({ auth, request: { nextUrl } }) {
      const path = nextUrl.pathname;
      if (protectedPages.some((p) => path === p || path.startsWith(`${p}/`))) return Boolean(auth?.user);
      return true;
    },
    session({ session, token }) {
      if (token.sub) session.user.id = token.sub;
      return session;
    },
  },
});
