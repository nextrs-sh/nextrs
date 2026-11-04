import { createHash } from "node:crypto";

// Consumes the request body incrementally through the Web stream Next.js
// builds from the Node request. ?mode=stream echoes it back as it arrives.
export async function POST(request: Request) {
  const mode = new URL(request.url).searchParams.get("mode");
  if (mode === "stream") {
    return new Response(request.body, {
      headers: { "content-type": "application/octet-stream" },
    });
  }
  const hash = createHash("sha256");
  let bytes = 0;
  let reads = 0;
  const reader = request.body?.getReader();
  if (reader) {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      hash.update(value);
      bytes += value.byteLength;
      reads++;
    }
  }
  return Response.json({ bytes, sha256: hash.digest("hex"), chunked: reads > 1 });
}
