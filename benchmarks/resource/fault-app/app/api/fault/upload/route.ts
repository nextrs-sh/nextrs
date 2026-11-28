// Large uploads (faults-large-uploads). By default the body is read
// incrementally and only counted (a route that streams); with ?buffer=1 it
// is read whole with arrayBuffer() first (a route that buffers).
export const dynamic = "force-dynamic";

export async function POST(request: Request) {
  const started = Date.now();
  if (new URL(request.url).searchParams.has("buffer")) {
    const body = await request.arrayBuffer();
    return Response.json({ mode: "buffer", bytes: body.byteLength, ms: Date.now() - started });
  }
  let bytes = 0;
  const reader = request.body?.getReader();
  if (reader) {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      bytes += value.byteLength;
    }
  }
  return Response.json({ mode: "stream", bytes, ms: Date.now() - started });
}
