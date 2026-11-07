import { currentSession } from "../../lib/dal";

export const dynamic = "force-dynamic";

// A route handler authorizes on its own: it does not trust the proxy.
export async function GET() {
  const session = await currentSession();
  if (!session) return Response.json({ error: "unauthorized" }, { status: 401 });
  return Response.json({ userId: session.userId, name: session.name });
}
