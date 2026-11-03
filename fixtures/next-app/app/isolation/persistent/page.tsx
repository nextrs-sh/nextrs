import { unstable_cache } from "next/cache";
import { headers } from "next/headers";

// unstable_cache is meant to persist across requests: the first request
// for a key stores its own identity, later requests for that key read it.
export default async function PersistentPage() {
  const list = await headers();
  const id = list.get("x-nextrs-id") ?? "(none)";
  const key = list.get("x-nextrs-key") ?? "default";
  const entry = await unstable_cache(async () => ({ key, createdBy: id }), ["nextrs-persistent", key], {
    revalidate: 3600,
  })();
  return (
    <main>
      <p id="entry">
        key={entry.key} createdBy={entry.createdBy} requestedBy={id}
      </p>
    </main>
  );
}
