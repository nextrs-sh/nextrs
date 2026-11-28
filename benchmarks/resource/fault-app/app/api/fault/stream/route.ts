// Slow response streams (faults-slow-streams): ?size= bytes (default
// 64 MiB) through a pull-based ReadableStream, produced only as fast as the
// client reads (as fixtures/next-app/app/api/stream). What a stalled
// reader costs is what the runtime buffers between this stream and the
// socket.
export const dynamic = "force-dynamic";

export async function GET(request: Request) {
  const size = Number(new URL(request.url).searchParams.get("size") ?? 64 * 1024 * 1024);
  const chunk = 16 * 1024;
  let sent = 0;
  let index = 0;
  const body = new ReadableStream<Uint8Array>({
    pull(controller) {
      if (sent >= size) {
        controller.close();
        return;
      }
      const length = Math.min(chunk, size - sent);
      controller.enqueue(new Uint8Array(length).fill(index % 251));
      sent += length;
      index++;
    },
  });
  return new Response(body, { headers: { "content-type": "application/octet-stream" } });
}
