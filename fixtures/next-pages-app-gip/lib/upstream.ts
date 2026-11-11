// The deterministic upstream the compatibility suites run
// (tests/compat/upstream.mjs); NEXTRS_UPSTREAM is passed to both servers.
export const upstream = () => process.env.NEXTRS_UPSTREAM ?? "http://127.0.0.1:4999";
export const delay = (ms: number) => new Promise((resolve) => setTimeout(resolve, ms));
// Request-derived values, never module state: each render returns its own.
export function cookieValue(header: string | undefined, name: string): string | null {
  for (const part of (header ?? "").split(";")) {
    const [key, ...rest] = part.trim().split("=");
    if (key === name) return decodeURIComponent(rest.join("="));
  }
  return null;
}
