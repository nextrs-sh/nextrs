import { renderData } from "../../lib/isr";

// A route handler with ISR: its response is cached (APP_ROUTE) and
// regenerated 2 s after each generation.
export const revalidate = 2;

export async function GET() {
  const data = await renderData("isr-route");
  return Response.json({ stamp: Date.now(), hit: data.hit });
}
