export const dynamic = "force-dynamic";

export function GET() {
  const state = (globalThis as { __platform?: unknown }).__platform ?? null;
  return Response.json({ state });
}
