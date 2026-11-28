// Native memory exhaustion (faults-native-memory). Each request allocates
// ?mib= MiB with Buffer.alloc (an ArrayBuffer backing store: outside the
// V8 heap, counted by --js-external-memory-limit), filled so every page is
// resident, and with ?hold=1 (the default) keeps it for the life of the
// isolate. ?release=1 drops everything held.
//
// 200 {"ok":true,"heldMiB":N}       allocated
// 500 {"caught":"RangeError",...}   refused inside JavaScript (the isolate lives)
export const dynamic = "force-dynamic";

const held: Buffer[] = [];
let heldBytes = 0;

export async function GET(request: Request) {
  const q = new URL(request.url).searchParams;
  if (q.has("release")) {
    const released = held.length;
    held.length = 0;
    heldBytes = 0;
    return Response.json({ released, heldMiB: 0 });
  }
  const mib = Number(q.get("mib") ?? "16");
  const hold = q.get("hold") !== "0";
  const started = Date.now();
  try {
    const buffer = Buffer.alloc(mib * 1024 * 1024, 1);
    if (hold) {
      held.push(buffer);
      heldBytes += buffer.length;
    }
    return Response.json({ ok: true, heldMiB: heldBytes / 1048576, ms: Date.now() - started });
  } catch (error) {
    const e = error as Error;
    return Response.json(
      { caught: e?.name ?? String(error), message: String(e?.message ?? "").slice(0, 200), heldMiB: heldBytes / 1048576 },
      { status: 500 },
    );
  }
}
