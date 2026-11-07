export const dynamic = "force-dynamic";

// An error the instrumentation hook's onRequestError sees.
export function GET() {
  throw new Error("route failure (synthetic)");
}
