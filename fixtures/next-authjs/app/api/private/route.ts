import { auth } from "../../../auth";

// A route handler that authorizes on its own, with Auth.js's wrapper.
export const GET = auth((req) => {
  if (!req.auth?.user) return Response.json({ error: "unauthorized" }, { status: 401 });
  return Response.json({ userId: req.auth.user.id, name: req.auth.user.name, email: req.auth.user.email });
});
