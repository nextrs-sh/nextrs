export const dynamic = "force-dynamic";

// A streamed body for the authenticated subject: `chunks` lines, `gap` ms
// apart, each naming the subject (an identity check across awaits).
export async function GET(req: Request) {
  const url = new URL(req.url);
  const chunks = Math.min(Number(url.searchParams.get("chunks") ?? 8), 64);
  const gap = Math.min(Number(url.searchParams.get("gap") ?? 5), 200);
  const sub = req.headers.get("x-auth-sub") ?? "anonymous";
  const encoder = new TextEncoder();
  const body = new ReadableStream({
    async start(controller) {
      for (let i = 0; i < chunks; i++) {
        controller.enqueue(encoder.encode(`${sub} ${i}\n`));
        await new Promise((resolve) => setTimeout(resolve, gap));
      }
      controller.close();
    },
  });
  return new Response(body, { headers: { "content-type": "text/plain" } });
}
