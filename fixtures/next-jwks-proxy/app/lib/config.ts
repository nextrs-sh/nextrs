// Run-time configuration (the test runner generates every value per run).
export const AUDIENCE = "nextrs-api";
// The algorithms the application accepts from its provider: never what a
// token's header claims on its own.
export const PROVIDER_ALGORITHMS = ["RS256", "PS256", "ES256", "ES384", "EdDSA"];

function required(name: string) {
  const value = process.env[name];
  if (!value) throw new Error(`${name} is not set`);
  return value;
}

// Tenants with their own issuer: a multi-tenant API picks the provider by
// path. `unlisted` and `metadata` are issuers NextRS's network policy does
// not allow (an unlisted port, a cloud metadata address).
export function issuerFor(tenant: string): string | null {
  switch (tenant) {
    case "main":
      return required("OIDC_ISSUER");
    case "unlisted":
      return `http://127.0.0.1:${required("NEXTRS_UNLISTED_PORT")}`;
    case "metadata":
      return "http://169.254.169.254";
    default:
      return null;
  }
}

type KeyPem = { private: string; public: string };
export function appKeys(): Record<string, KeyPem> {
  return JSON.parse(required("APP_KEYS"));
}
