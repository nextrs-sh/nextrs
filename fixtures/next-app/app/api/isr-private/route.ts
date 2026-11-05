// Declares ISR but reads the request's authorization: Next treats it as
// dynamic, so nothing is cached between callers.
export const revalidate = 60;

export async function GET(request: Request) {
  return Response.json({ auth: request.headers.get("authorization"), stamp: Date.now() });
}
