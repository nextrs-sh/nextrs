export async function GET(request: Request) {
  const url = new URL(request.url);
  return Response.json({ router: "app", q: url.searchParams.get("q") });
}
