// Synthetic test identities (no real accounts or credentials).
export const USERS: Record<string, { userId: string; name: string; email: string; password: string }> = {
  alice: { userId: "u-alice", name: "Alice", email: "alice@example.test", password: "alice-test-password" },
  bob: { userId: "u-bob", name: "Bob", email: "bob@example.test", password: "bob-test-password" },
};
