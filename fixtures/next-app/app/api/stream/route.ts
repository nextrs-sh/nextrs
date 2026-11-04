// Streams ?size= bytes (default 8 MiB) through a pull-based ReadableStream,
// so it produces only as fast as the client reads.
export async function GET(request: Request) {
  const size = Number(new URL(request.url).searchParams.get("size") ?? 8 * 1024 * 1024);
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
      const bytes = new Uint8Array(length).fill(index % 251);
      new DataView(bytes.buffer).setUint32(0, index);
      controller.enqueue(bytes);
      sent += length;
      index++;
    },
  });
  return new Response(body, { headers: { "content-type": "application/octet-stream" } });
}
