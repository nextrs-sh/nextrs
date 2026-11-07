// Prints fresh test signing keys for fixtures/next-jwks-proxy as the
// APP_KEYS JSON ({alg: {private: PKCS#8 PEM, public: SPKI PEM}}). Generated
// per run; nothing persists them.
import { generateKeyPairSync } from "node:crypto";

const pem = ({ privateKey, publicKey }) => ({
  private: privateKey.export({ type: "pkcs8", format: "pem" }),
  public: publicKey.export({ type: "spki", format: "pem" }),
});
console.log(
  JSON.stringify({
    ES256: pem(generateKeyPairSync("ec", { namedCurve: "P-256" })),
    PS256: pem(generateKeyPairSync("rsa", { modulusLength: 2048 })),
    RS256: pem(generateKeyPairSync("rsa", { modulusLength: 2048 })),
    EdDSA: pem(generateKeyPairSync("ed25519")),
  }),
);
