"use server";

// The soak driver calls it with its own token and checks the echo.
export async function soakEcho(token: string) {
  await new Promise((r) => setTimeout(r, 5));
  return `echo:${token}`;
}
