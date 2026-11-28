// The readiness path the harness waits for (the same as Velasia's), so
// series1/lib.mjs storefrontUp() starts this app like the storefront.
export const dynamic = "force-dynamic";

export async function GET() {
  return Response.json({ status: "ready", service: "nextrs-resource-fault-app" });
}
