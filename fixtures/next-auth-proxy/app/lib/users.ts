// Synthetic test identities (no real accounts or credentials).
export const USERS: Record<string, { userId: string; name: string; password: string }> = {
  alice: { userId: "u-alice", name: "Alice", password: "alice-test-password" },
  bob: { userId: "u-bob", name: "Bob", password: "bob-test-password" },
};

// Only same-origin paths: `next` comes from the query string.
export function safeNext(next: unknown) {
  const value = typeof next === "string" ? next : "";
  if (!value.startsWith("/") || value.startsWith("//") || value.includes("\\")) return "/dashboard";
  return value;
}
