import { png } from "./data";

// Image bytes from a route handler: the optimizer's source request goes
// through the server's routing to this dynamic route.
export async function GET() {
  return new Response(Buffer.from(png, "base64"), {
    headers: { "content-type": "image/png", "cache-control": "public, max-age=60" },
  });
}
