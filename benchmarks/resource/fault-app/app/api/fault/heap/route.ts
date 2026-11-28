// One large JavaScript allocation (faults-js-allocation), the modes of
// crates/nextrs-runtime/tests/fixtures/handlers/dist/heap-faults.js:
//   ?mode=heap-single&mib=N   one JS-heap object of N MiB (a FixedDoubleArray)
//   ?mode=heap-string&mib=N   one flat one-byte string of N MiB
// The object is kept, so the heap stays full. A heap limit is not a
// catchable error: V8 terminates the isolate (NextRS replaces it) or
// aborts the process (Node.js: "JavaScript heap out of memory").
export const dynamic = "force-dynamic";

const hold: unknown[] = [];

export async function GET(request: Request) {
  const q = new URL(request.url).searchParams;
  const mode = q.get("mode") ?? "heap-single";
  const mib = Number(q.get("mib") ?? "64");
  const started = Date.now();
  try {
    if (mode === "heap-single") {
      hold.push(new Array(mib * 131072).fill(0.5));
    } else if (mode === "heap-string") {
      // repeat() may build a cons string; toUpperCase() allocates one flat
      // sequential one-byte string of the full length.
      hold.push("a".repeat(mib * 1024 * 1024).toUpperCase());
    } else {
      return Response.json({ error: `unknown mode ${mode}` }, { status: 400 });
    }
  } catch (error) {
    return Response.json({ caught: (error as Error)?.name ?? String(error), ms: Date.now() - started }, { status: 500 });
  }
  return Response.json({ ok: true, mode, mib, held: hold.length, ms: Date.now() - started });
}
