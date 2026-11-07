import { draftMode } from "next/headers";

// Only the listed posts exist (dynamicParams = false); Draft Mode renders
// them on demand instead of serving the prerender.
export const dynamicParams = false;

export function generateStaticParams() {
  return [{ slug: "a" }, { slug: "b" }];
}

export default async function Post({ params }: { params: Promise<{ slug: string }> }) {
  const { slug } = await params;
  const { isEnabled } = await draftMode();
  return (
    <main>
      <h1 id="title">Post</h1>
      <p id="slug">{slug}</p>
      <p id="kind">{isEnabled ? "draft" : "published"}</p>
      <p>stamp-{Date.now()}</p>
    </main>
  );
}
